"""Training: holdout quality, saved artefacts and the command line."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from analytics import model as model_module
from analytics.features import FEATURES
from analytics.model import DelayRiskModel, load_or_train
from analytics.train import main, metadata_path_for


def test_quick_model_beats_the_auc_floor(loaded_model: DelayRiskModel) -> None:
    metadata = loaded_model.metadata
    assert metadata["quick"] is True
    assert metadata["auc"] > 0.70, metadata
    assert metadata["brier"] < 0.25, metadata
    assert metadata["features"] == list(FEATURES)
    assert metadata["calibration"] == "sigmoid"
    assert (
        metadata["n_train"] + metadata["n_calibration"] + metadata["n_holdout"]
        == (metadata["n_rows"])
    )


def test_cli_writes_model_and_metadata(tmp_path: Path) -> None:
    out = tmp_path / "nested" / "delay_risk.joblib"
    assert main(["--quick", "--out", str(out), "--rows", "2500"]) == 0
    assert out.is_file()
    metadata = json.loads(metadata_path_for(out).read_text(encoding="utf-8"))
    for key in ("trained_at", "n_rows", "auc", "brier", "features", "version"):
        assert key in metadata
    assert metadata["n_rows"] == 2500
    model = DelayRiskModel.load(out)
    assert model.version == metadata["version"]


def test_load_or_train_falls_back_to_a_quick_model(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    # Hide the image-bundled model so the fallback path is the only one available.
    monkeypatch.setattr(model_module, "BUNDLED_MODEL_PATH", tmp_path / "bundled" / "none.joblib")
    missing = tmp_path / "does-not-exist" / "delay_risk.joblib"
    model = load_or_train(missing)
    assert model.metadata["quick"] is True
    assert missing.is_file(), "the on-the-fly model should be persisted for the next start"
