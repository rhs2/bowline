"""Token guard, probes and problem documents."""

from __future__ import annotations

import pytest
from fastapi.testclient import TestClient

from analytics.app import create_app
from analytics.config import Settings
from analytics.problems import PROBLEM_MEDIA_TYPE

SCORE_BODY = {
    "mode": "road",
    "weight_kg": 1200,
    "pieces": 4,
    "hazardous": False,
    "etd": "2026-09-01",
    "eta": "2026-09-04",
}


def test_missing_token_is_rejected_with_problem_json(client: TestClient) -> None:
    response = client.post("/score/delay-risk", json=SCORE_BODY)
    assert response.status_code == 401
    assert response.headers["content-type"].startswith(PROBLEM_MEDIA_TYPE)
    body = response.json()
    assert body["code"] == "unauthorized"
    assert body["status"] == 401
    assert body["title"] == "Unauthorized"
    assert body["request_id"]


def test_wrong_token_is_rejected(client: TestClient) -> None:
    response = client.post(
        "/score/delay-risk", json=SCORE_BODY, headers={"X-Internal-Token": "nope"}
    )
    assert response.status_code == 401
    assert response.json()["code"] == "unauthorized"


def test_forecast_routes_require_token(client: TestClient) -> None:
    assert client.get("/forecast/volume").status_code == 401
    assert client.post("/forecast/volume", json={"series": []}).status_code == 401


def test_probes_do_not_require_token(client: TestClient) -> None:
    health = client.get("/healthz")
    assert health.status_code == 200
    assert health.json()["status"] == "ok"
    assert health.json()["model_loaded"] is True
    assert health.json()["model_version"].startswith("delay-risk-")

    metrics = client.get("/metrics")
    assert metrics.status_code == 200
    assert "bowline_analytics_http_requests_total" in metrics.text
    assert "bowline_analytics_http_request_duration_seconds" in metrics.text
    assert "bowline_analytics_model_loaded 1.0" in metrics.text


def test_request_id_is_echoed_or_generated(client: TestClient) -> None:
    echoed = client.get("/healthz", headers={"X-Request-Id": "req-123"})
    assert echoed.headers["x-request-id"] == "req-123"
    generated = client.get("/healthz")
    assert len(generated.headers["x-request-id"]) == 32


def test_unknown_route_returns_problem_json(client: TestClient, headers: dict[str, str]) -> None:
    response = client.get("/nope", headers=headers)
    assert response.status_code == 404
    assert response.headers["content-type"].startswith(PROBLEM_MEDIA_TYPE)
    assert response.json()["code"] == "not_found"


def test_app_refuses_to_start_without_token(tmp_path) -> None:
    settings = Settings(
        INTERNAL_SERVICE_TOKEN="",
        ANALYTICS_MODEL_PATH=str(tmp_path / "delay_risk.joblib"),
        ANALYTICS_DATABASE_URL="",
        ANALYTICS_LOG_LEVEL="WARNING",
    )
    with pytest.raises(RuntimeError, match="INTERNAL_SERVICE_TOKEN"):
        create_app(settings)
