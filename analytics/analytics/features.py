"""The feature contract shared by training and scoring."""

from __future__ import annotations

import math
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import date
from typing import Any

import pandas as pd

FEATURES: tuple[str, ...] = (
    "mode",
    "distance_km",
    "weight_kg",
    "pieces",
    "hazardous",
    "carrier_on_time_rate",
    "month",
    "lead_days",
    "customs",
)
CATEGORICAL_FEATURES: tuple[str, ...] = ("mode",)
NUMERIC_FEATURES: tuple[str, ...] = tuple(f for f in FEATURES if f not in CATEGORICAL_FEATURES)
LABEL = "delayed"

# Modes whose shipments cross a border and clear customs unless the caller says otherwise.
CUSTOMS_BY_DEFAULT: frozenset[str] = frozenset({"sea", "air"})
MAX_LEAD_DAYS = 366


@dataclass(frozen=True)
class ShipmentFeatures:
    """One shipment, ready for the model."""

    mode: str
    distance_km: float | None
    weight_kg: float
    pieces: int
    hazardous: bool
    carrier_on_time_rate: float | None
    month: int
    lead_days: int
    customs: bool

    def as_row(self) -> dict[str, Any]:
        return {
            "mode": self.mode,
            "distance_km": _nan_if_none(self.distance_km),
            "weight_kg": float(self.weight_kg),
            "pieces": float(self.pieces),
            "hazardous": float(self.hazardous),
            "carrier_on_time_rate": _nan_if_none(self.carrier_on_time_rate),
            "month": float(self.month),
            "lead_days": float(self.lead_days),
            "customs": float(self.customs),
        }


def _nan_if_none(value: float | None) -> float:
    return math.nan if value is None else float(value)


def derive_features(
    *,
    mode: str,
    weight_kg: float,
    pieces: int,
    hazardous: bool,
    distance_km: float | None,
    carrier_on_time_rate: float | None,
    etd: date,
    eta: date,
    customs: bool | None = None,
) -> ShipmentFeatures:
    """Turn a booking into model features: lead time, departure month and the customs flag."""
    if eta < etd:
        raise ValueError("eta must be on or after etd")
    lead_days = (eta - etd).days
    if lead_days > MAX_LEAD_DAYS:
        raise ValueError(f"lead time must be at most {MAX_LEAD_DAYS} days")
    return ShipmentFeatures(
        mode=mode,
        distance_km=distance_km,
        weight_kg=weight_kg,
        pieces=pieces,
        hazardous=hazardous,
        carrier_on_time_rate=carrier_on_time_rate,
        month=etd.month,
        lead_days=lead_days,
        customs=(mode in CUSTOMS_BY_DEFAULT) if customs is None else customs,
    )


def feature_frame(frame: pd.DataFrame) -> pd.DataFrame:
    """Select and type the model columns from a wider frame (for example the synthetic data)."""
    out = frame.loc[:, list(FEATURES)].copy()
    out["mode"] = out["mode"].astype(object)
    for column in NUMERIC_FEATURES:
        out[column] = out[column].astype(float)
    return out


def frame_from_rows(rows: Iterable[dict[str, Any]]) -> pd.DataFrame:
    """Build a model-ready frame from `ShipmentFeatures.as_row()` dictionaries."""
    return feature_frame(pd.DataFrame(list(rows), columns=list(FEATURES)))
