"""Shared fixtures. No database and no network: the model is trained once per session."""

from __future__ import annotations

import math
from collections.abc import Iterator
from datetime import date, timedelta
from pathlib import Path

import pytest
from fastapi.testclient import TestClient

from analytics.app import create_app
from analytics.config import Settings
from analytics.forecast import WeekCount
from analytics.history import HistoryUnavailableError
from analytics.model import DelayRiskModel
from analytics.train import save_bundle, train_model

TEST_TOKEN = "test-internal-token"


class FakeHistorySource:
    """Stands in for the database: returns a fixed series or raises."""

    name = "database"

    def __init__(self, series: list[WeekCount] | None) -> None:
        self.series = series
        self.calls: list[str | None] = []

    def fetch(self, site: str | None) -> list[WeekCount]:
        self.calls.append(site)
        if self.series is None:
            raise HistoryUnavailableError("the shipment history database is not reachable")
        return list(self.series)


def weekly_series(weeks: int, *, start: date = date(2025, 1, 6), base: float = 40.0) -> list[dict]:
    """A deterministic weekly series with trend and yearly seasonality (Monday week starts)."""
    series = []
    for index in range(weeks):
        seasonal = 8.0 * math.sin(2.0 * math.pi * index / 52.0)
        count = max(0, round(base + 0.3 * index + seasonal + (index % 3) - 1))
        series.append({"week_start": (start + timedelta(weeks=index)).isoformat(), "count": count})
    return series


@pytest.fixture(scope="session")
def model_path(tmp_path_factory: pytest.TempPathFactory) -> Path:
    path = tmp_path_factory.mktemp("model") / "delay_risk.joblib"
    save_bundle(train_model(quick=True), path)
    return path


@pytest.fixture(scope="session")
def loaded_model(model_path: Path) -> DelayRiskModel:
    return DelayRiskModel.load(model_path)


@pytest.fixture(scope="session")
def settings(model_path: Path) -> Settings:
    return Settings(
        INTERNAL_SERVICE_TOKEN=TEST_TOKEN,
        ANALYTICS_MODEL_PATH=str(model_path),
        ANALYTICS_DATABASE_URL="",
        ANALYTICS_BIND_PORT=8082,
        LOG_FORMAT="json",
        ANALYTICS_LOG_LEVEL="WARNING",
    )


@pytest.fixture(scope="session")
def client(settings: Settings, loaded_model: DelayRiskModel) -> Iterator[TestClient]:
    app = create_app(settings, model=loaded_model)
    with TestClient(app) as test_client:
        yield test_client


@pytest.fixture
def headers() -> dict[str, str]:
    return {"X-Internal-Token": TEST_TOKEN}
