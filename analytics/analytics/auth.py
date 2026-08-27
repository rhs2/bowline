"""Internal service authentication.

The API calls this service with the shared secret from `INTERNAL_SERVICE_TOKEN` in the
`X-Internal-Token` header. Every route except the health and metrics probes requires it.
"""

from __future__ import annotations

import hmac

from fastapi import Request

from analytics.problems import ProblemError

TOKEN_HEADER = "X-Internal-Token"


def require_internal_token(request: Request) -> None:
    """FastAPI dependency: reject the request unless the internal token matches."""
    expected: str = request.app.state.settings.internal_service_token
    presented = request.headers.get(TOKEN_HEADER, "")
    if not expected or not presented:
        raise ProblemError(401, "unauthorized", f"a valid {TOKEN_HEADER} header is required")
    if not hmac.compare_digest(presented.encode("utf-8"), expected.encode("utf-8")):
        raise ProblemError(401, "unauthorized", f"a valid {TOKEN_HEADER} header is required")
