"""Synthetic shipment generator: determinism, label rate and the documented latent effects."""

from __future__ import annotations

import numpy as np
import pandas as pd

from analytics.features import FEATURES, LABEL
from analytics.synthetic import MODES, generate, latent_logit


def test_same_seed_same_frame() -> None:
    first = generate(2_000, seed=7)
    second = generate(2_000, seed=7)
    pd.testing.assert_frame_equal(first, second)


def test_different_seed_different_frame() -> None:
    assert not generate(500, seed=1).equals(generate(500, seed=2))


def test_columns_and_types() -> None:
    frame = generate(300, seed=3)
    for column in (*FEATURES, "latent_risk", LABEL):
        assert column in frame.columns
    assert set(frame["mode"].unique()) <= set(MODES)
    assert frame[LABEL].dtype == bool
    assert frame["pieces"].min() >= 1
    assert frame["lead_days"].min() >= 0
    assert frame["distance_km"].isna().mean() > 0.02
    assert frame["carrier_on_time_rate"].isna().mean() > 0.05
    assert frame["carrier_on_time_rate"].dropna().between(0, 1).all()


def test_label_rate_is_realistic() -> None:
    frame = generate(20_000, seed=42)
    rate = frame[LABEL].mean()
    assert 0.10 <= rate <= 0.45, rate


def test_latent_effects_have_the_documented_direction() -> None:
    frame = generate(20_000, seed=42)
    by_mode = frame.groupby("mode")["latent_risk"].mean()
    assert by_mode["sea"] > by_mode["air"]
    assert frame.loc[frame["customs"], "latent_risk"].mean() > (
        frame.loc[~frame["customs"], "latent_risk"].mean()
    )
    by_month = frame.groupby("month")["latent_risk"].mean()
    assert by_month[12] > by_month[6]


def test_latent_logit_moves_with_carrier_rate_and_lead_time() -> None:
    def logit(rate: float, lead: float) -> float:
        return float(
            latent_logit(
                mode=np.array(["sea"], dtype=object),
                distance_km=np.array([9000.0]),
                weight_kg=np.array([8000.0]),
                pieces=np.array([10]),
                hazardous=np.array([False]),
                carrier_on_time_rate=np.array([rate]),
                month=np.array([6]),
                lead_days=np.array([lead]),
                customs=np.array([True]),
            )[0]
        )

    assert logit(0.95, 30) < logit(0.70, 30)
    assert logit(0.85, 30) < logit(0.85, 10)
