"""POST /score/delay-risk."""

from __future__ import annotations

from datetime import date
from typing import Any

import numpy as np
import pandas as pd
from fastapi.testclient import TestClient

from analytics.features import FEATURES, derive_features
from analytics.model import RISK_CEILING, RISK_FLOOR, DelayRiskModel, band_for

SEA_RISKY = {
    "mode": "sea",
    "weight_kg": 18_000,
    "pieces": 40,
    "hazardous": True,
    "distance_km": 11_000,
    "carrier_on_time_rate": 0.60,
    "etd": "2026-12-01",
    "eta": "2026-12-06",
}

AIR_SAFE = {
    "mode": "air",
    "weight_kg": 200,
    "pieces": 2,
    "hazardous": False,
    "distance_km": 4_000,
    "carrier_on_time_rate": 0.97,
    "etd": "2026-06-01",
    "eta": "2026-06-21",
}


def test_score_shape(client: TestClient, headers: dict[str, str]) -> None:
    response = client.post("/score/delay-risk", json=SEA_RISKY, headers=headers)
    assert response.status_code == 200, response.text
    body = response.json()
    assert 0.0 <= body["risk"] <= 1.0
    assert body["band"] in {"low", "medium", "high"}
    assert body["band"] == band_for(body["risk"])
    assert body["model_version"].startswith("delay-risk-")
    assert len(body["drivers"]) == 3
    for driver in body["drivers"]:
        assert driver["feature"] in FEATURES
        assert driver["direction"] in {"increases", "decreases", "neutral"}
    assert body["derived"] == {"lead_days": 5, "month": 12, "customs": True}


def test_monotonic_sanity(client: TestClient, headers: dict[str, str]) -> None:
    risky = client.post("/score/delay-risk", json=SEA_RISKY, headers=headers).json()
    safe = client.post("/score/delay-risk", json=AIR_SAFE, headers=headers).json()
    assert risky["risk"] > safe["risk"], (risky, safe)
    assert risky["band"] != "low"
    assert safe["band"] != "high"


def test_drivers_point_the_right_way_for_a_risky_sea_shipment(
    client: TestClient, headers: dict[str, str]
) -> None:
    body = client.post("/score/delay-risk", json=SEA_RISKY, headers=headers).json()
    increasing = {d["feature"] for d in body["drivers"] if d["direction"] == "increases"}
    assert increasing & {"carrier_on_time_rate", "lead_days", "mode", "month"}


def test_optional_fields_can_be_omitted_and_customs_derived(
    client: TestClient, headers: dict[str, str]
) -> None:
    body = {
        "mode": "road",
        "weight_kg": 900,
        "pieces": 3,
        "hazardous": False,
        "etd": "2026-03-02T08:00:00Z",
        "eta": "2026-03-04T17:30:00+02:00",
    }
    response = client.post("/score/delay-risk", json=body, headers=headers)
    assert response.status_code == 200, response.text
    assert response.json()["derived"] == {"lead_days": 2, "month": 3, "customs": False}


def test_customs_override(client: TestClient, headers: dict[str, str]) -> None:
    without = client.post(
        "/score/delay-risk", json={**AIR_SAFE, "customs": False}, headers=headers
    ).json()
    assert without["derived"]["customs"] is False


def test_validation_errors_are_problem_documents(
    client: TestClient, headers: dict[str, str]
) -> None:
    response = client.post(
        "/score/delay-risk", json={**AIR_SAFE, "eta": "2026-05-01"}, headers=headers
    )
    assert response.status_code == 422
    body = response.json()
    assert body["code"] == "validation_failed"
    assert any("eta" in error["message"] for error in body["errors"])

    response = client.post("/score/delay-risk", json={**AIR_SAFE, "mode": "camel"}, headers=headers)
    assert response.status_code == 422
    assert response.json()["errors"][0]["field"] == "mode"


def test_scores_are_counted(client: TestClient, headers: dict[str, str]) -> None:
    client.post("/score/delay-risk", json=AIR_SAFE, headers=headers)
    metrics = client.get("/metrics").text
    assert "bowline_analytics_scores_served_total" in metrics


class _SaturatingEstimator:
    """A calibrated estimator that has saturated: the shipment scores 1.0, variants 0.0.

    Isotonic calibration really does return exactly 1.0 when the outermost bin of the
    calibration split is pure, which the 50,000-row training run reaches.
    """

    def predict_proba(self, frame: pd.DataFrame) -> Any:
        positive = np.zeros(len(frame))
        positive[0] = 1.0
        return np.column_stack([1.0 - positive, positive])


def test_saturated_probabilities_are_held_inside_the_open_interval() -> None:
    model = DelayRiskModel(
        {
            "model": _SaturatingEstimator(),
            "features": list(FEATURES),
            "baselines": {
                "mode": "road",
                "by_mode": {},
                "global": dict.fromkeys(FEATURES, 0.0),
            },
            "metadata": {"version": "delay-risk-test"},
        }
    )
    shipment = derive_features(
        mode="sea",
        weight_kg=18_000,
        pieces=40,
        hazardous=True,
        distance_km=11_000,
        carrier_on_time_rate=0.60,
        etd=date(2026, 12, 1),
        eta=date(2026, 12, 6),
    )
    result = model.score(shipment)
    assert result.risk == RISK_CEILING
    assert result.band == "high"
    # Every variant saturated at the floor, so each driver spans the whole clamped range.
    assert all(driver.delta == round(RISK_CEILING - RISK_FLOOR, 4) for driver in result.drivers)
