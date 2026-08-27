"""Train, evaluate, calibrate and save the delay-risk model.

    python -m analytics.train            # full training run (default 50,000 synthetic rows)
    python -m analytics.train --quick    # small run for CI and for on-the-fly fallback
    python -m analytics.train --out /path/to/delay_risk.joblib

The estimator is a scikit-learn Pipeline: a ColumnTransformer that one-hot encodes `mode`
and passes the numeric features through (HistGradientBoosting handles NaN natively), then a
HistGradientBoostingClassifier. The data is split three ways (train / calibration /
holdout). The pipeline is fitted on the train split, probabilities are calibrated on the
calibration split (isotonic for a full run, sigmoid for `--quick`, where isotonic would
overfit), and AUC and Brier score are reported on the untouched holdout.
"""

from __future__ import annotations

import argparse
import json
import logging
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

import joblib
import numpy as np
import pandas as pd
import sklearn
from sklearn.calibration import CalibratedClassifierCV
from sklearn.compose import ColumnTransformer
from sklearn.ensemble import HistGradientBoostingClassifier
from sklearn.frozen import FrozenEstimator
from sklearn.metrics import brier_score_loss, roc_auc_score
from sklearn.model_selection import train_test_split
from sklearn.pipeline import Pipeline
from sklearn.preprocessing import OneHotEncoder

from analytics import __version__
from analytics.config import Settings
from analytics.features import FEATURES, LABEL, NUMERIC_FEATURES, feature_frame
from analytics.logging_setup import configure_logging
from analytics.synthetic import DEFAULT_SEED, MODES, generate

log = logging.getLogger("analytics.train")

MODEL_FAMILY = "delay-risk"
FEATURE_SCHEMA_VERSION = 1
FULL_ROWS = 50_000
QUICK_ROWS = 5_000
HOLDOUT_SHARE = 0.15
CALIBRATION_SHARE = 0.20  # of the non-holdout rows
REFERENCE_MODE = "road"
MODE_SPECIFIC_BASELINES: tuple[str, ...] = ("distance_km", "weight_kg", "pieces", "lead_days")


@dataclass
class TrainedBundle:
    """Everything the service needs at scoring time, in one joblib file."""

    model: CalibratedClassifierCV
    features: list[str]
    baselines: dict[str, Any]
    metadata: dict[str, Any] = field(default_factory=dict)

    def as_dict(self) -> dict[str, Any]:
        return {
            "model": self.model,
            "features": list(self.features),
            "baselines": self.baselines,
            "metadata": self.metadata,
        }


def build_pipeline(*, quick: bool, seed: int) -> Pipeline:
    preprocess = ColumnTransformer(
        [
            ("mode", OneHotEncoder(handle_unknown="ignore", sparse_output=False), ["mode"]),
            ("numeric", "passthrough", list(NUMERIC_FEATURES)),
        ],
        verbose_feature_names_out=False,
    )
    classifier = HistGradientBoostingClassifier(
        max_iter=120 if quick else 300,
        learning_rate=0.06,
        max_leaf_nodes=15,
        min_samples_leaf=25,
        l2_regularization=0.5,
        early_stopping=False,
        random_state=seed,
    )
    return Pipeline([("features", preprocess), ("classifier", classifier)])


def compute_baselines(frame: pd.DataFrame) -> dict[str, Any]:
    """Reference values used to explain a score as per-feature deltas.

    Distance, weight, pieces and lead time are compared with the median of the shipment's own
    mode; the carrier rate and month with the global median; the flags with `False`; and
    the mode with the reference mode.
    """
    global_medians = {
        feature: float(frame[feature].median(skipna=True))
        for feature in NUMERIC_FEATURES
        if feature not in MODE_SPECIFIC_BASELINES
    }
    global_medians["hazardous"] = 0.0
    global_medians["customs"] = 0.0
    by_mode: dict[str, dict[str, float]] = {}
    for mode in MODES:
        subset = frame.loc[frame["mode"] == mode]
        source = subset if len(subset) else frame
        by_mode[mode] = {
            feature: float(source[feature].median(skipna=True))
            for feature in MODE_SPECIFIC_BASELINES
        }
    return {"mode": REFERENCE_MODE, "global": global_medians, "by_mode": by_mode}


def train_model(
    *, quick: bool = False, n_rows: int | None = None, seed: int = DEFAULT_SEED
) -> TrainedBundle:
    """Train and evaluate a delay-risk model on synthetic shipments."""
    rows = n_rows or (QUICK_ROWS if quick else FULL_ROWS)
    started = datetime.now(UTC)
    data = generate(rows, seed=seed)
    x_all = feature_frame(data)
    y_all = data[LABEL].to_numpy(dtype=int)

    x_fit, x_holdout, y_fit, y_holdout = train_test_split(
        x_all, y_all, test_size=HOLDOUT_SHARE, stratify=y_all, random_state=seed
    )
    x_train, x_calibration, y_train, y_calibration = train_test_split(
        x_fit, y_fit, test_size=CALIBRATION_SHARE, stratify=y_fit, random_state=seed
    )

    pipeline = build_pipeline(quick=quick, seed=seed).fit(x_train, y_train)
    raw_holdout = pipeline.predict_proba(x_holdout)[:, 1]
    raw_auc = float(roc_auc_score(y_holdout, raw_holdout))
    raw_brier = float(brier_score_loss(y_holdout, raw_holdout))

    calibration_method = "sigmoid" if quick else "isotonic"
    calibrated = CalibratedClassifierCV(FrozenEstimator(pipeline), method=calibration_method)
    calibrated.fit(x_calibration, y_calibration)
    holdout_probability = calibrated.predict_proba(x_holdout)[:, 1]
    auc = float(roc_auc_score(y_holdout, holdout_probability))
    brier = float(brier_score_loss(y_holdout, holdout_probability))

    trained_at = datetime.now(UTC)
    version = f"{MODEL_FAMILY}-{FEATURE_SCHEMA_VERSION}.{trained_at:%Y%m%d%H%M%S}"
    metadata: dict[str, Any] = {
        "version": version,
        "model_family": MODEL_FAMILY,
        "feature_schema_version": FEATURE_SCHEMA_VERSION,
        "service_version": __version__,
        "trained_at": trained_at.isoformat(timespec="seconds"),
        "training_seconds": round((trained_at - started).total_seconds(), 3),
        "quick": quick,
        "seed": seed,
        "n_rows": int(rows),
        "n_train": len(x_train),
        "n_calibration": len(x_calibration),
        "n_holdout": len(x_holdout),
        "label_rate": round(float(np.mean(y_all)), 4),
        "auc": round(auc, 4),
        "brier": round(brier, 4),
        "raw_auc": round(raw_auc, 4),
        "raw_brier": round(raw_brier, 4),
        "calibration": calibration_method,
        "features": list(FEATURES),
        "estimator": "HistGradientBoostingClassifier",
        "sklearn_version": sklearn.__version__,
    }
    log.info(
        "delay-risk model trained",
        extra={
            "version": version,
            "quick": quick,
            "n_rows": rows,
            "auc": metadata["auc"],
            "brier": metadata["brier"],
            "raw_auc": metadata["raw_auc"],
            "calibration": calibration_method,
            "training_seconds": metadata["training_seconds"],
        },
    )
    return TrainedBundle(
        model=calibrated,
        features=list(FEATURES),
        baselines=compute_baselines(x_train),
        metadata=metadata,
    )


def metadata_path_for(model_path: Path) -> Path:
    return model_path.with_suffix(".json")


def save_bundle(bundle: TrainedBundle, model_path: Path) -> Path:
    """Write the joblib bundle and a metadata JSON next to it; returns the model path."""
    model_path = Path(model_path)
    model_path.parent.mkdir(parents=True, exist_ok=True)
    joblib.dump(bundle.as_dict(), model_path, compress=3)
    metadata_path_for(model_path).write_text(
        json.dumps(bundle.metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    log.info(
        "delay-risk model saved",
        extra={"model_path": str(model_path), "metadata_path": str(metadata_path_for(model_path))},
    )
    return model_path


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="python -m analytics.train",
        description="Train the Bowline delay-risk model on synthetic shipments.",
    )
    parser.add_argument(
        "--quick",
        action="store_true",
        help=f"train on {QUICK_ROWS:,} rows with sigmoid calibration (seconds, for CI)",
    )
    parser.add_argument(
        "--out",
        type=Path,
        default=None,
        help="model file to write (default: ANALYTICS_MODEL_PATH or ./models/delay_risk.joblib)",
    )
    parser.add_argument("--rows", type=int, default=None, help="override the number of rows")
    parser.add_argument("--seed", type=int, default=DEFAULT_SEED, help="random seed")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    settings = Settings()
    configure_logging(settings.log_format, settings.log_level)
    out = args.out or settings.model_path
    bundle = train_model(quick=args.quick, n_rows=args.rows, seed=args.seed)
    save_bundle(bundle, out)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
