"""Loading and using the delay-risk model at scoring time."""

from __future__ import annotations

import logging
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Literal

import joblib
import numpy as np

from analytics.features import FEATURES, ShipmentFeatures, frame_from_rows

log = logging.getLogger("analytics.model")

Band = Literal["low", "medium", "high"]
Direction = Literal["increases", "decreases", "neutral"]

MEDIUM_THRESHOLD = 0.30
HIGH_THRESHOLD = 0.60
TOP_DRIVERS = 3

# Isotonic calibration returns exactly 0.0 or 1.0 whenever the outermost bin of the
# calibration split happened to be pure, and the full training run does hit that: a sea
# booking with a poor carrier and a short lead time came back as a flat 1.0. Reporting
# certainty that a shipment will or will not be late is never warranted, and a caller
# that takes the log of the score would divide by zero, so scores are held just inside
# the open interval. The clamp is applied to the whole batch, so the driver deltas stay
# consistent with the risk they explain.
RISK_FLOOR = 0.001
RISK_CEILING = 0.999
BUNDLED_MODEL_PATH = Path(__file__).resolve().parent.parent / "models" / "delay_risk.joblib"


@dataclass(frozen=True)
class Driver:
    feature: str
    direction: Direction
    delta: float


@dataclass(frozen=True)
class ScoreResult:
    risk: float
    band: Band
    model_version: str
    drivers: list[Driver]


def band_for(risk: float) -> Band:
    if risk >= HIGH_THRESHOLD:
        return "high"
    if risk >= MEDIUM_THRESHOLD:
        return "medium"
    return "low"


class DelayRiskModel:
    """A loaded model bundle with scoring and cheap per-feature explanations."""

    def __init__(self, bundle: dict[str, Any]) -> None:
        missing = {"model", "features", "baselines", "metadata"} - set(bundle)
        if missing:
            raise ValueError(f"model bundle is missing {sorted(missing)}")
        if list(bundle["features"]) != list(FEATURES):
            raise ValueError("model bundle was trained with a different feature schema")
        self._model = bundle["model"]
        self.features: list[str] = list(bundle["features"])
        self.baselines: dict[str, Any] = bundle["baselines"]
        self.metadata: dict[str, Any] = dict(bundle["metadata"])

    @classmethod
    def load(cls, path: Path) -> DelayRiskModel:
        return cls(joblib.load(path))

    @property
    def version(self) -> str:
        return str(self.metadata.get("version", "unknown"))

    def predict(self, rows: list[dict[str, Any]]) -> np.ndarray:
        frame = frame_from_rows(rows)
        return self._model.predict_proba(frame)[:, 1]

    def baseline_for(self, feature: str, mode: str) -> Any:
        if feature == "mode":
            return self.baselines["mode"]
        by_mode = self.baselines["by_mode"].get(mode) or {}
        if feature in by_mode:
            return by_mode[feature]
        return self.baselines["global"][feature]

    def score(self, shipment: ShipmentFeatures) -> ScoreResult:
        """Score one shipment and explain it.

        Drivers come from a per-feature delta: each feature is replaced in turn by its baseline
        value (see `train.compute_baselines`) and the change in calibrated risk is recorded.
        All variants are scored in one batch, so the cost is a single predict call.
        """
        actual = shipment.as_row()
        variants = [actual]
        for feature in self.features:
            variant = dict(actual)
            variant[feature] = self.baseline_for(feature, shipment.mode)
            variants.append(variant)
        probabilities = np.clip(self.predict(variants), RISK_FLOOR, RISK_CEILING)
        risk = float(probabilities[0])

        deltas = [
            (feature, risk - float(probabilities[index + 1]))
            for index, feature in enumerate(self.features)
        ]
        deltas.sort(key=lambda item: abs(item[1]), reverse=True)
        drivers = [
            Driver(feature=feature, direction=_direction(delta), delta=round(delta, 4))
            for feature, delta in deltas[:TOP_DRIVERS]
        ]
        return ScoreResult(
            risk=round(risk, 4), band=band_for(risk), model_version=self.version, drivers=drivers
        )


def _direction(delta: float, tolerance: float = 1e-4) -> Direction:
    if delta > tolerance:
        return "increases"
    if delta < -tolerance:
        return "decreases"
    return "neutral"


def candidate_paths(configured: Path) -> list[Path]:
    """Where to look for a model: the configured path first, then the image-bundled one."""
    candidates = [Path(configured)]
    if BUNDLED_MODEL_PATH.resolve() != Path(configured).resolve():
        candidates.append(BUNDLED_MODEL_PATH)
    return candidates


def load_or_train(configured: Path) -> DelayRiskModel:
    """Load the model from disk, or train a quick one on the fly when none exists."""
    for path in candidate_paths(configured):
        if path.is_file():
            model = DelayRiskModel.load(path)
            log.info(
                "delay-risk model loaded",
                extra={
                    "model_path": str(path),
                    "model_version": model.version,
                    "auc": model.metadata.get("auc"),
                    "brier": model.metadata.get("brier"),
                },
            )
            return model

    log.warning(
        "delay-risk model file not found; training a quick model on the fly",
        extra={"model_path": str(configured)},
    )
    from analytics.train import save_bundle, train_model

    bundle = train_model(quick=True)
    try:
        save_bundle(bundle, Path(configured))
    except OSError as exc:
        log.warning(
            "could not persist the on-the-fly model; keeping it in memory only",
            extra={"model_path": str(configured), "error": str(exc)},
        )
    return DelayRiskModel(bundle.as_dict())
