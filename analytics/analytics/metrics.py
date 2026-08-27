"""Prometheus metrics exposed at /metrics."""

from __future__ import annotations

from prometheus_client import (
    CONTENT_TYPE_LATEST,
    REGISTRY,
    Counter,
    Gauge,
    Histogram,
    Info,
    generate_latest,
)

HTTP_REQUESTS = Counter(
    "bowline_analytics_http_requests_total",
    "HTTP requests handled, by method, route template and status code",
    ["method", "route", "status"],
)

HTTP_LATENCY = Histogram(
    "bowline_analytics_http_request_duration_seconds",
    "HTTP request latency in seconds, by method and route template",
    ["method", "route"],
    buckets=(0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0),
)

SCORES_SERVED = Counter(
    "bowline_analytics_scores_served_total",
    "Delay-risk scores served, by risk band",
    ["band"],
)

FORECASTS_SERVED = Counter(
    "bowline_analytics_forecasts_served_total",
    "Volume forecasts served, by history source and method",
    ["source", "method"],
)

MODEL_LOADED = Gauge(
    "bowline_analytics_model_loaded",
    "1 when a delay-risk model is loaded and scoring is available",
)

MODEL_INFO = Info(
    "bowline_analytics_model",
    "Version and training summary of the loaded delay-risk model",
)


def render_metrics() -> tuple[bytes, str]:
    """Return the exposition body and its content type."""
    return generate_latest(REGISTRY), CONTENT_TYPE_LATEST
