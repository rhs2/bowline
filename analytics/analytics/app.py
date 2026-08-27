"""Application factory."""

from __future__ import annotations

import asyncio
import logging
from collections.abc import AsyncIterator
from contextlib import asynccontextmanager

from fastapi import FastAPI

from analytics import __version__
from analytics.config import Settings
from analytics.history import HistorySource, build_history_source
from analytics.logging_setup import configure_logging
from analytics.metrics import MODEL_INFO, MODEL_LOADED
from analytics.middleware import RequestContextMiddleware
from analytics.model import DelayRiskModel, load_or_train
from analytics.problems import install_exception_handlers
from analytics.routes import internal, probes

log = logging.getLogger("analytics.app")


def create_app(
    settings: Settings | None = None,
    *,
    model: DelayRiskModel | None = None,
    history_source: HistorySource | None = None,
) -> FastAPI:
    """Build the FastAPI application.

    `model` and `history_source` can be injected (tests do); otherwise the model is loaded
    from `ANALYTICS_MODEL_PATH` (or trained on the fly) and the history source follows
    `ANALYTICS_DATABASE_URL`.
    """
    settings = settings or Settings()
    configure_logging(settings.log_format, settings.log_level)
    if not settings.token_configured:
        raise RuntimeError(
            "INTERNAL_SERVICE_TOKEN must be set; the analytics service refuses to start without it"
        )

    @asynccontextmanager
    async def lifespan(app: FastAPI) -> AsyncIterator[None]:
        loaded = (
            model
            if model is not None
            else await asyncio.to_thread(load_or_train, settings.model_path)
        )
        app.state.model = loaded
        app.state.history_source = (
            history_source if history_source is not None else build_history_source(settings)
        )
        MODEL_LOADED.set(1)
        MODEL_INFO.info(
            {
                "version": loaded.version,
                "trained_at": str(loaded.metadata.get("trained_at", "")),
                "auc": str(loaded.metadata.get("auc", "")),
                "brier": str(loaded.metadata.get("brier", "")),
            }
        )
        log.info(
            "analytics service ready",
            extra={
                "service_version": __version__,
                "model_version": loaded.version,
                "history_source": app.state.history_source.name,
                "bind_port": settings.bind_port,
            },
        )
        try:
            yield
        finally:
            MODEL_LOADED.set(0)
            log.info("analytics service stopping")

    app = FastAPI(
        title="Bowline analytics",
        version=__version__,
        description="Shipment delay-risk scoring and weekly volume forecasting.",
        lifespan=lifespan,
        docs_url=None,
        redoc_url=None,
        openapi_url=None,
    )
    app.state.settings = settings
    app.add_middleware(RequestContextMiddleware)
    install_exception_handlers(app)
    app.include_router(probes)
    app.include_router(internal)
    return app
