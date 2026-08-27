"""Deterministic synthetic shipment records.

A fresh Bowline installation has no delay history, so the delay-risk model is trained on
synthetic shipments whose delay probability follows a documented latent structure:

* sea freight and customs clearance raise the risk; air is the most reliable mode;
* a carrier with a high on-time rate lowers the risk;
* a lead time (ETD to ETA) that is short for the mode raises the risk;
* hazardous cargo, long distances and heavy or fragmented consignments add a little risk;
* seasonality peaks in December and troughs in June.

The `delayed` label is then sampled from that probability after adding Gaussian noise on the
logit scale, so no model can separate the classes perfectly. Every draw comes from a seeded
`numpy` generator: the same `n_rows` and `seed` always produce the same frame.
"""

from __future__ import annotations

import numpy as np
import pandas as pd

DEFAULT_SEED = 42

MODES: tuple[str, ...] = ("sea", "air", "road", "rail")
MODE_WEIGHTS: tuple[float, ...] = (0.40, 0.20, 0.30, 0.10)

# Per-mode shape of the observable features.
DISTANCE_MEDIAN_KM = {"sea": 9000.0, "air": 4500.0, "road": 500.0, "rail": 1500.0}
DISTANCE_SIGMA = {"sea": 0.5, "air": 0.6, "road": 0.8, "rail": 0.6}
WEIGHT_MEDIAN_KG = {"sea": 8000.0, "air": 250.0, "road": 1500.0, "rail": 20000.0}
WEIGHT_SIGMA = {"sea": 1.0, "air": 1.2, "road": 1.0, "rail": 0.8}
PIECES_LAMBDA = {"sea": 12.0, "air": 4.0, "road": 6.0, "rail": 20.0}
TYPICAL_LEAD_DAYS = {"sea": 30.0, "air": 5.0, "road": 3.0, "rail": 10.0}
MIN_LEAD_DAYS = {"sea": 5.0, "air": 1.0, "road": 0.0, "rail": 2.0}
CUSTOMS_SHARE = {"sea": 0.95, "air": 0.90, "road": 0.25, "rail": 0.40}
HAZARDOUS_SHARE = 0.08
CARRIER_RATE_BETA = (17.0, 3.0)  # mean 0.85, standard deviation about 0.08

# Latent delay model, on the logit scale.
INTERCEPT = -4.0
MODE_EFFECT = {"sea": 1.4, "air": -0.4, "road": 0.0, "rail": 0.4}
CUSTOMS_EFFECT = 0.9
CARRIER_RATE_EFFECT = -9.0  # per unit of (on_time_rate - 0.85)
SHORT_LEAD_EFFECT = 3.0  # times clip(1 - lead_days / typical_lead, -0.5, 1)
HAZARDOUS_EFFECT = 0.5
DISTANCE_EFFECT = 0.25  # times log1p(distance_km / 1000)
WEIGHT_EFFECT = 0.08  # times log1p(weight_kg / 1000)
PIECES_EFFECT = 0.05  # times log1p(pieces)
SEASON_EFFECT = 1.6  # times a cosine that is 1 in December and 0 in June
NOISE_SD = 0.4

# Share of rows where an optional feature is unknown at booking time.
MISSING_DISTANCE_SHARE = 0.10
MISSING_CARRIER_RATE_SHARE = 0.15


def _per_mode(values: dict[str, float], mode: np.ndarray) -> np.ndarray:
    """Look up a per-mode constant for every row of `mode`."""
    lookup = np.array([values[name] for name in MODES], dtype=float)
    codes = pd.Categorical(np.asarray(mode, dtype=object), categories=list(MODES)).codes
    if (codes < 0).any():
        raise ValueError(f"unknown transport mode; expected one of {MODES}")
    return lookup[codes]


def sigmoid(x: np.ndarray) -> np.ndarray:
    return 1.0 / (1.0 + np.exp(-x))


def seasonal_factor(month: np.ndarray) -> np.ndarray:
    """1.0 in December, 0.0 in June, smooth in between."""
    return 0.5 * (1.0 + np.cos(2.0 * np.pi * (np.asarray(month, dtype=float) - 12.0) / 12.0))


def latent_logit(
    mode: np.ndarray,
    distance_km: np.ndarray,
    weight_kg: np.ndarray,
    pieces: np.ndarray,
    hazardous: np.ndarray,
    carrier_on_time_rate: np.ndarray,
    month: np.ndarray,
    lead_days: np.ndarray,
    customs: np.ndarray,
) -> np.ndarray:
    """The noise-free delay logit for fully observed shipments."""
    typical = _per_mode(TYPICAL_LEAD_DAYS, mode)
    lead_shortfall = np.clip(1.0 - np.asarray(lead_days, dtype=float) / typical, -0.5, 1.0)
    return (
        INTERCEPT
        + _per_mode(MODE_EFFECT, mode)
        + CUSTOMS_EFFECT * np.asarray(customs, dtype=float)
        + CARRIER_RATE_EFFECT * (np.asarray(carrier_on_time_rate, dtype=float) - 0.85)
        + SHORT_LEAD_EFFECT * lead_shortfall
        + HAZARDOUS_EFFECT * np.asarray(hazardous, dtype=float)
        + DISTANCE_EFFECT * np.log1p(np.asarray(distance_km, dtype=float) / 1000.0)
        + WEIGHT_EFFECT * np.log1p(np.asarray(weight_kg, dtype=float) / 1000.0)
        + PIECES_EFFECT * np.log1p(np.asarray(pieces, dtype=float))
        + SEASON_EFFECT * seasonal_factor(month)
    )


def generate(n_rows: int, seed: int = DEFAULT_SEED) -> pd.DataFrame:
    """Generate `n_rows` shipments. Same arguments, same frame, on every platform.

    Columns: mode, distance_km, weight_kg, pieces, hazardous, carrier_on_time_rate, month,
    lead_days, customs, latent_risk, delayed. `distance_km` and `carrier_on_time_rate`
    contain NaN for a share of rows, as they would at booking time.
    """
    if n_rows <= 0:
        raise ValueError("n_rows must be positive")
    rng = np.random.default_rng(seed)

    mode_index = rng.choice(len(MODES), size=n_rows, p=MODE_WEIGHTS)
    mode = np.array(MODES, dtype=object)[mode_index]

    distance_km = np.exp(
        rng.normal(np.log(_per_mode(DISTANCE_MEDIAN_KM, mode)), _per_mode(DISTANCE_SIGMA, mode))
    )
    weight_kg = np.exp(
        rng.normal(np.log(_per_mode(WEIGHT_MEDIAN_KG, mode)), _per_mode(WEIGHT_SIGMA, mode))
    )
    pieces = 1 + rng.poisson(_per_mode(PIECES_LAMBDA, mode))
    hazardous = rng.random(n_rows) < HAZARDOUS_SHARE
    carrier_on_time_rate = rng.beta(*CARRIER_RATE_BETA, size=n_rows)
    customs = rng.random(n_rows) < _per_mode(CUSTOMS_SHARE, mode)
    month = rng.integers(1, 13, size=n_rows)

    typical_lead = _per_mode(TYPICAL_LEAD_DAYS, mode)
    lead_days = np.maximum(
        _per_mode(MIN_LEAD_DAYS, mode),
        np.rint(rng.normal(typical_lead, 0.35 * typical_lead)),
    ).astype(int)

    logit = latent_logit(
        mode,
        distance_km,
        weight_kg,
        pieces,
        hazardous,
        carrier_on_time_rate,
        month,
        lead_days,
        customs,
    )
    latent_risk = sigmoid(logit + rng.normal(0.0, NOISE_SD, size=n_rows))
    delayed = rng.random(n_rows) < latent_risk

    observed_distance = distance_km.copy()
    observed_distance[rng.random(n_rows) < MISSING_DISTANCE_SHARE] = np.nan
    observed_rate = carrier_on_time_rate.copy()
    observed_rate[rng.random(n_rows) < MISSING_CARRIER_RATE_SHARE] = np.nan

    return pd.DataFrame(
        {
            "mode": mode,
            "distance_km": np.round(observed_distance, 1),
            "weight_kg": np.round(weight_kg, 1),
            "pieces": pieces.astype(int),
            "hazardous": hazardous,
            "carrier_on_time_rate": np.round(observed_rate, 4),
            "month": month.astype(int),
            "lead_days": lead_days,
            "customs": customs,
            "latent_risk": latent_risk,
            "delayed": delayed,
        }
    )
