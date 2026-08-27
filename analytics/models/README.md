# models/

This directory holds the trained delay-risk model (`delay_risk.joblib`) and its metadata
(`delay_risk.json`). Both files are build artefacts and are ignored by git.

- The Docker image runs `python -m analytics.train` at build time, so every image ships
  with a model trained from the deterministic synthetic generator.
- Locally, `python -m analytics.train` (or `--quick`) writes the files here.
- If the service starts and finds no model at `ANALYTICS_MODEL_PATH`, it trains a quick
  model on the fly, logs a warning and saves it here for the next start.

The metadata JSON records the version, training time, row counts, holdout AUC and Brier
score, calibration method and feature list of the model next to it.
