"""HTTP routes."""

from __future__ import annotations

import logging

from fastapi import APIRouter, Depends, Query, Request, Response

from analytics.auth import require_internal_token
from analytics.features import derive_features
from analytics.forecast import (
    InsufficientHistoryError,
    SeriesError,
    WeekCount,
    forecast_volume,
)
from analytics.history import HistoryUnavailableError
from analytics.metrics import FORECASTS_SERVED, SCORES_SERVED, render_metrics
from analytics.model import DelayRiskModel
from analytics.problems import ProblemError, request_id_of
from analytics.schemas import (
    DerivedOut,
    DriverOut,
    ForecastWeekOut,
    HealthResponse,
    ScoreRequest,
    ScoreResponse,
    VolumeForecastResponse,
    VolumeSeriesRequest,
)

log = logging.getLogger("analytics.routes")

probes = APIRouter(tags=["probes"])
internal = APIRouter(dependencies=[Depends(require_internal_token)], tags=["internal"])

WeeksQuery = Query(8, ge=1, le=52, description="Forecast horizon in weeks")
SiteQuery = Query(None, min_length=1, max_length=120, description="Filter by origin city")


@probes.get("/healthz", response_model=HealthResponse)
def healthz(request: Request) -> HealthResponse:
    model: DelayRiskModel | None = getattr(request.app.state, "model", None)
    return HealthResponse(
        status="ok",
        model_loaded=model is not None,
        model_version=model.version if model is not None else None,
    )


@probes.get("/metrics", include_in_schema=False)
def metrics() -> Response:
    body, content_type = render_metrics()
    return Response(content=body, media_type=content_type)


@internal.post("/score/delay-risk", response_model=ScoreResponse)
def score_delay_risk(body: ScoreRequest, request: Request) -> ScoreResponse:
    model: DelayRiskModel = request.app.state.model
    shipment = derive_features(
        mode=body.mode.value,
        weight_kg=body.weight_kg,
        pieces=body.pieces,
        hazardous=body.hazardous,
        distance_km=body.distance_km,
        carrier_on_time_rate=body.carrier_on_time_rate,
        etd=body.etd,
        eta=body.eta,
        customs=body.customs,
    )
    result = model.score(shipment)
    SCORES_SERVED.labels(result.band).inc()
    log.info(
        "delay risk scored",
        extra={
            "request_id": request_id_of(request),
            "mode": shipment.mode,
            "lead_days": shipment.lead_days,
            "risk": result.risk,
            "band": result.band,
            "model_version": result.model_version,
        },
    )
    return ScoreResponse(
        risk=result.risk,
        band=result.band,
        model_version=result.model_version,
        drivers=[
            DriverOut(feature=d.feature, direction=d.direction, delta=d.delta)
            for d in result.drivers
        ],
        derived=DerivedOut(
            lead_days=shipment.lead_days, month=shipment.month, customs=shipment.customs
        ),
    )


@internal.get("/forecast/volume", response_model=VolumeForecastResponse)
def forecast_volume_from_history(
    request: Request,
    weeks: int = WeeksQuery,
    site: str | None = SiteQuery,
) -> VolumeForecastResponse:
    source = request.app.state.history_source
    try:
        series = source.fetch(site)
    except HistoryUnavailableError as exc:
        raise ProblemError(
            503,
            "history_unavailable",
            f"{exc}; POST /forecast/volume with a weekly series instead",
        ) from exc
    return _forecast(series, weeks, source="database", request=request)


@internal.post("/forecast/volume", response_model=VolumeForecastResponse)
def forecast_volume_from_series(
    body: VolumeSeriesRequest,
    request: Request,
    weeks: int = WeeksQuery,
) -> VolumeForecastResponse:
    series = [WeekCount(week_start=item.week_start, count=item.count) for item in body.series]
    return _forecast(series, weeks, source="request", request=request)


def _forecast(
    series: list[WeekCount], weeks: int, *, source: str, request: Request
) -> VolumeForecastResponse:
    try:
        result = forecast_volume(series, weeks)
    except InsufficientHistoryError as exc:
        raise ProblemError(422, "insufficient_history", str(exc)) from exc
    except SeriesError as exc:
        raise ProblemError(422, "validation_failed", str(exc)) from exc
    FORECASTS_SERVED.labels(source, result.method).inc()
    log.info(
        "volume forecast served",
        extra={
            "request_id": request_id_of(request),
            "source": source,
            "method": result.method,
            "history_weeks": result.history_weeks,
            "horizon": weeks,
        },
    )
    return VolumeForecastResponse(
        weeks=[
            ForecastWeekOut(week_start=w.week_start, point=w.point, low=w.low, high=w.high)
            for w in result.weeks
        ],
        method=result.method,
        history_weeks=result.history_weeks,
        source=source,  # type: ignore[arg-type]
    )
