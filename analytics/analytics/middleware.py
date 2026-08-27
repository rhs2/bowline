"""Request context middleware: request ids, access log and HTTP metrics.

Implemented as a plain ASGI callable so it never buffers response bodies and keeps working
for streaming responses.
"""

from __future__ import annotations

import logging
import re
import time
import uuid
from collections.abc import Awaitable, Callable, MutableMapping
from typing import Any

from starlette.datastructures import Headers, MutableHeaders
from starlette.routing import Match

from analytics.metrics import HTTP_LATENCY, HTTP_REQUESTS

log = logging.getLogger("analytics.access")

Scope = MutableMapping[str, Any]
Message = MutableMapping[str, Any]
Receive = Callable[[], Awaitable[Message]]
Send = Callable[[Message], Awaitable[None]]
ASGIApp = Callable[[Scope, Receive, Send], Awaitable[None]]

REQUEST_ID_HEADER = "x-request-id"
_REQUEST_ID_PATTERN = re.compile(r"[A-Za-z0-9._:-]{1,64}")


def _incoming_request_id(headers: Headers) -> str:
    candidate = headers.get(REQUEST_ID_HEADER, "")
    if candidate and _REQUEST_ID_PATTERN.fullmatch(candidate):
        return candidate
    return uuid.uuid4().hex


def route_template(scope: Scope) -> str:
    """The route path template (for metric labels), or `unmatched` for unknown paths."""
    route = scope.get("route")
    path = getattr(route, "path", None)
    if isinstance(path, str):
        return path
    app = scope.get("app")
    for candidate in getattr(app, "routes", []):
        match, _ = candidate.matches(scope)
        if match == Match.FULL:
            return str(candidate.path)
    return "unmatched"


class RequestContextMiddleware:
    """Attach a request id, record metrics and write one access-log line per request."""

    def __init__(self, app: ASGIApp) -> None:
        self.app = app

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return

        request_id = _incoming_request_id(Headers(scope=scope))
        scope.setdefault("state", {})["request_id"] = request_id
        method = str(scope.get("method", "GET"))
        status = 500
        started = time.perf_counter()

        async def send_with_request_id(message: Message) -> None:
            nonlocal status
            if message["type"] == "http.response.start":
                status = int(message["status"])
                MutableHeaders(scope=message).append(REQUEST_ID_HEADER, request_id)
            await send(message)

        try:
            await self.app(scope, receive, send_with_request_id)
        finally:
            elapsed = time.perf_counter() - started
            route = route_template(scope)
            HTTP_REQUESTS.labels(method, route, str(status)).inc()
            HTTP_LATENCY.labels(method, route).observe(elapsed)
            log.info(
                "request",
                extra={
                    "request_id": request_id,
                    "method": method,
                    "path": scope.get("path", ""),
                    "route": route,
                    "status": status,
                    "latency_ms": round(elapsed * 1000.0, 2),
                },
            )
