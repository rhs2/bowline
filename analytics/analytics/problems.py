"""RFC 7807 problem documents, matching the error contract of the Bowline API."""

from __future__ import annotations

import logging
from typing import Any

from fastapi import FastAPI, Request
from fastapi.exceptions import RequestValidationError
from fastapi.responses import JSONResponse
from starlette.exceptions import HTTPException as StarletteHTTPException

log = logging.getLogger("analytics.http")

PROBLEM_MEDIA_TYPE = "application/problem+json"

_TITLES: dict[int, str] = {
    400: "Bad Request",
    401: "Unauthorized",
    403: "Forbidden",
    404: "Not Found",
    405: "Method Not Allowed",
    409: "Conflict",
    422: "Unprocessable Content",
    429: "Too Many Requests",
    500: "Internal Server Error",
    503: "Service Unavailable",
}

_CODES: dict[int, str] = {
    400: "bad_request",
    401: "unauthorized",
    403: "forbidden",
    404: "not_found",
    405: "method_not_allowed",
    409: "conflict",
    422: "validation_failed",
    429: "rate_limited",
    500: "internal",
    503: "unavailable",
}


class ProblemError(Exception):
    """Raise anywhere in a handler to return a problem document with a stable code."""

    def __init__(
        self,
        status: int,
        code: str,
        detail: str,
        *,
        title: str | None = None,
        errors: list[dict[str, str]] | None = None,
    ) -> None:
        super().__init__(detail)
        self.status = status
        self.code = code
        self.detail = detail
        self.title = title
        self.errors = errors


def request_id_of(request: Request) -> str:
    return str(getattr(request.state, "request_id", "") or "")


def problem_response(
    status: int,
    code: str,
    detail: str,
    request_id: str,
    *,
    title: str | None = None,
    errors: list[dict[str, str]] | None = None,
) -> JSONResponse:
    body: dict[str, Any] = {
        "type": "about:blank",
        "title": title or _TITLES.get(status, "Error"),
        "status": status,
        "detail": detail,
        "code": code,
        "request_id": request_id,
    }
    if errors is not None:
        body["errors"] = errors
    return JSONResponse(body, status_code=status, media_type=PROBLEM_MEDIA_TYPE)


def _validation_errors(exc: RequestValidationError) -> list[dict[str, str]]:
    errors: list[dict[str, str]] = []
    for item in exc.errors():
        location = [str(part) for part in item.get("loc", ()) if part not in ("body",)]
        errors.append({"field": ".".join(location) or "body", "message": str(item.get("msg", ""))})
    return errors


def install_exception_handlers(app: FastAPI) -> None:
    """Convert every error path into a problem document."""

    @app.exception_handler(ProblemError)
    async def _problem(request: Request, exc: ProblemError) -> JSONResponse:
        return problem_response(
            exc.status,
            exc.code,
            exc.detail,
            request_id_of(request),
            title=exc.title,
            errors=exc.errors,
        )

    @app.exception_handler(RequestValidationError)
    async def _validation(request: Request, exc: RequestValidationError) -> JSONResponse:
        return problem_response(
            422,
            "validation_failed",
            "request validation failed",
            request_id_of(request),
            errors=_validation_errors(exc),
        )

    @app.exception_handler(StarletteHTTPException)
    async def _http(request: Request, exc: StarletteHTTPException) -> JSONResponse:
        status = exc.status_code
        detail = exc.detail if isinstance(exc.detail, str) else _TITLES.get(status, "error")
        response = problem_response(
            status, _CODES.get(status, "error"), detail, request_id_of(request)
        )
        if exc.headers:
            response.headers.update(exc.headers)
        return response

    @app.exception_handler(Exception)
    async def _unexpected(request: Request, exc: Exception) -> JSONResponse:
        log.exception(
            "unhandled error",
            extra={"request_id": request_id_of(request), "path": request.url.path},
        )
        return problem_response(500, "internal", "internal error", request_id_of(request))
