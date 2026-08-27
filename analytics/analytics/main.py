"""ASGI entry point: `uvicorn analytics.main:app` or `python -m analytics.main`."""

from __future__ import annotations

import uvicorn

from analytics.app import create_app

app = create_app()


def serve() -> None:
    """Run the service with the configured port; logging is already set up by `create_app`."""
    settings = app.state.settings
    uvicorn.run(
        app,
        host="0.0.0.0",  # container binding; the ALB and the compose network front it
        port=settings.bind_port,
        log_config=None,
        access_log=False,
    )


if __name__ == "__main__":
    serve()
