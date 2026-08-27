"""Volume forecast: the algorithm and both endpoints."""

from __future__ import annotations

from datetime import date, timedelta

import pytest
from fastapi.testclient import TestClient

from analytics.app import create_app
from analytics.config import Settings
from analytics.forecast import (
    METHOD_BLEND,
    METHOD_SEASONAL_BLEND,
    METHOD_TREND,
    InsufficientHistoryError,
    WeekCount,
    forecast_volume,
    normalise_series,
)
from analytics.model import DelayRiskModel
from tests.conftest import FakeHistorySource, weekly_series


def _assert_well_formed(body: dict, horizon: int, last_week: date) -> None:
    assert len(body["weeks"]) == horizon
    expected = last_week + timedelta(weeks=1)
    for week in body["weeks"]:
        assert week["week_start"] == expected.isoformat()
        assert 0 <= week["low"] <= week["point"] <= week["high"]
        expected += timedelta(weeks=1)


def test_post_series_returns_requested_weeks(client: TestClient, headers: dict[str, str]) -> None:
    series = weekly_series(30)
    response = client.post("/forecast/volume?weeks=8", json={"series": series}, headers=headers)
    assert response.status_code == 200, response.text
    body = response.json()
    _assert_well_formed(body, 8, date.fromisoformat(series[-1]["week_start"]))
    assert body["method"] == METHOD_BLEND
    assert body["history_weeks"] == 30
    assert body["source"] == "request"


def test_post_series_custom_horizon(client: TestClient, headers: dict[str, str]) -> None:
    series = weekly_series(40)
    response = client.post("/forecast/volume?weeks=12", json={"series": series}, headers=headers)
    assert response.status_code == 200
    assert len(response.json()["weeks"]) == 12


def test_short_series_uses_the_trend_fallback(client: TestClient, headers: dict[str, str]) -> None:
    response = client.post("/forecast/volume", json={"series": weekly_series(5)}, headers=headers)
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["method"] == METHOD_TREND
    _assert_well_formed(body, 8, date(2025, 1, 6) + timedelta(weeks=4))


def test_too_short_series_is_rejected(client: TestClient, headers: dict[str, str]) -> None:
    response = client.post("/forecast/volume", json={"series": weekly_series(1)}, headers=headers)
    assert response.status_code == 422
    body = response.json()
    assert body["code"] == "insufficient_history"
    assert "at least 3 weeks" in body["detail"]


def test_invalid_series_body(client: TestClient, headers: dict[str, str]) -> None:
    response = client.post(
        "/forecast/volume",
        json={"series": [{"week_start": "2025-01-06", "count": -1}]},
        headers=headers,
    )
    assert response.status_code == 422
    assert response.json()["code"] == "validation_failed"


def test_get_without_database_reports_history_unavailable(
    client: TestClient, headers: dict[str, str]
) -> None:
    response = client.get("/forecast/volume", headers=headers)
    assert response.status_code == 503
    body = response.json()
    assert body["code"] == "history_unavailable"
    assert "POST /forecast/volume" in body["detail"]


def test_get_reads_history_from_the_source(
    settings: Settings, loaded_model: DelayRiskModel
) -> None:
    series = [
        WeekCount(date.fromisoformat(item["week_start"]), item["count"])
        for item in weekly_series(60)
    ]
    source = FakeHistorySource(series)
    app = create_app(settings, model=loaded_model, history_source=source)
    with TestClient(app) as client:
        response = client.get(
            "/forecast/volume?weeks=4&site=Port%20City",
            headers={"X-Internal-Token": settings.internal_service_token},
        )
    assert response.status_code == 200, response.text
    body = response.json()
    assert body["source"] == "database"
    assert body["history_weeks"] == 60
    assert body["method"] == METHOD_SEASONAL_BLEND
    _assert_well_formed(body, 4, series[-1].week_start)
    assert source.calls == ["Port City"]


def test_get_with_unreachable_database(settings: Settings, loaded_model: DelayRiskModel) -> None:
    app = create_app(settings, model=loaded_model, history_source=FakeHistorySource(None))
    with TestClient(app) as client:
        response = client.get(
            "/forecast/volume", headers={"X-Internal-Token": settings.internal_service_token}
        )
    assert response.status_code == 503
    assert response.json()["code"] == "history_unavailable"


def test_get_with_empty_history(settings: Settings, loaded_model: DelayRiskModel) -> None:
    app = create_app(settings, model=loaded_model, history_source=FakeHistorySource([]))
    with TestClient(app) as client:
        response = client.get(
            "/forecast/volume", headers={"X-Internal-Token": settings.internal_service_token}
        )
    assert response.status_code == 422
    assert response.json()["code"] == "insufficient_history"


def test_normalise_aligns_sums_and_fills_gaps() -> None:
    weeks, values = normalise_series(
        [
            WeekCount(date(2025, 1, 8), 3),  # a Wednesday: aligned to Monday 2025-01-06
            WeekCount(date(2025, 1, 6), 2),
            WeekCount(date(2025, 1, 20), 5),  # 2025-01-13 is missing and becomes 0
        ]
    )
    assert weeks == [date(2025, 1, 6), date(2025, 1, 13), date(2025, 1, 20)]
    assert values.tolist() == [5.0, 0.0, 5.0]


def test_forecast_volume_direct() -> None:
    series = [
        WeekCount(date.fromisoformat(item["week_start"]), item["count"])
        for item in weekly_series(120)
    ]
    result = forecast_volume(series, 52)
    assert result.method == METHOD_SEASONAL_BLEND
    assert len(result.weeks) == 52
    assert all(w.low <= w.point <= w.high for w in result.weeks)
    # Intervals widen with the horizon.
    assert (result.weeks[-1].high - result.weeks[-1].low) > (
        result.weeks[0].high - result.weeks[0].low
    )
    # The series trends upward, and so should the forecast level.
    assert result.weeks[-1].point > series[0].count

    with pytest.raises(InsufficientHistoryError):
        forecast_volume(series[:2], 4)
