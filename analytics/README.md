# Bowline analytics

The Python service behind two operational questions:

- **Delay risk.** When a shipment is booked or a leg changes, the API asks for a 0 to 1
  probability that the shipment arrives late, and stores it on the shipment.
- **Volume forecast.** How many shipments to expect per week over the coming weeks, with
  an 80% interval, for capacity and shift planning.

FastAPI on port `ANALYTICS_BIND_PORT` (default 8082), scikit-learn for the models,
`psycopg` for the optional read-only database access, `prometheus-client` for metrics.
Everything installs from wheels on macOS arm64 and Linux; no native extras.

## Running it

```sh
cd analytics
python3 -m venv .venv && . .venv/bin/activate
python -m pip install -r requirements.txt -r requirements-dev.txt

python -m analytics.train --quick          # writes models/delay_risk.joblib and .json
INTERNAL_SERVICE_TOKEN=dev-internal-token-change-me uvicorn analytics.main:app --port 8082
```

`python -m analytics.main` does the same with the port taken from `ANALYTICS_BIND_PORT`
(that is what the container runs). The service refuses to start without
`INTERNAL_SERVICE_TOKEN`. If no model file exists it trains a quick one on the fly, logs
a warning and saves it for the next start.

Configuration (all from the environment, names as in the repository `.env.example`):

| Variable                 | Default                     | Meaning                                                   |
|--------------------------|-----------------------------|-----------------------------------------------------------|
| `ANALYTICS_BIND_PORT`    | `8082`                      | Listening port                                            |
| `ANALYTICS_MODEL_PATH`   | `./models/delay_risk.joblib`| Model bundle; the image-bundled model is the fallback     |
| `ANALYTICS_DATABASE_URL` | unset                       | Read-only Postgres DSN for shipment history (optional)    |
| `INTERNAL_SERVICE_TOKEN` | required                    | Shared secret expected in `X-Internal-Token`              |
| `LOG_FORMAT`             | `json`                      | `json` or `pretty`                                        |
| `ANALYTICS_LOG_LEVEL`    | `INFO`                      | Python log level                                          |

## Endpoints

Every route except `/healthz` and `/metrics` requires `X-Internal-Token` equal to
`INTERNAL_SERVICE_TOKEN`; anything else is a `401` problem document. Errors follow
RFC 7807 (`application/problem+json`) with a stable `code`, like the API:
`unauthorized`, `validation_failed` (with `errors: [{field, message}]`), `not_found`,
`insufficient_history`, `history_unavailable`, `internal`. Every response carries an
`X-Request-Id` (echoed from the request when present).

### POST /score/delay-risk

```sh
curl -s -X POST http://localhost:8082/score/delay-risk \
  -H 'X-Internal-Token: dev-internal-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{"mode":"sea","weight_kg":18000,"pieces":40,"hazardous":true,
       "distance_km":11000,"carrier_on_time_rate":0.6,
       "etd":"2026-12-01","eta":"2026-12-06"}'
```

```json
{
  "risk": 0.9183,
  "band": "high",
  "model_version": "delay-risk-1.20260826231547",
  "drivers": [
    {"feature": "mode", "direction": "increases", "delta": 0.2455},
    {"feature": "customs", "direction": "increases", "delta": 0.0767},
    {"feature": "lead_days", "direction": "increases", "delta": 0.0516}
  ],
  "derived": {"lead_days": 5, "month": 12, "customs": true}
}
```

The generator and the training split are seeded, so that request scores 0.9183 on any
machine; only `model_version` differs between builds, because it carries the training
timestamp.

`distance_km` and `carrier_on_time_rate` are optional (the model handles missing
values natively). Dates are `YYYY-MM-DD` or RFC 3339 timestamps. From the body the
service derives `lead_days` (ETA minus ETD), `month` (of the ETD) and `customs` (true
for sea and air unless the optional `customs` field says otherwise). Bands: `low`
below 0.30, `medium` from 0.30, `high` from 0.60.

`drivers` are the three features whose replacement by a baseline value moves the risk
the most: each feature is swapped in turn for a reference value (the median of the
shipment's mode for distance, weight, pieces and lead time; the global median for the
carrier rate and month; `false` for the flags; `road` for the mode) and the change in
calibrated risk is recorded. All variants are scored in one batch, so an explained
score costs a single prediction call.

Scores are held in `[0.001, 0.999]`. Isotonic calibration returns exactly 0.0 or 1.0
whenever the outermost bin of the calibration split is pure, and the full training run
does reach that, but certainty that a shipment will or will not be late is never
warranted and it would break any caller that takes the log of the score. The clamp is
applied to the whole batch, so the driver deltas stay consistent with the risk.

### GET /forecast/volume?weeks=8&site=

Weekly shipment counts are read from `shipments.created_at` through
`ANALYTICS_DATABASE_URL` (read-only role, one second connect timeout, three second
statement timeout), optionally filtered by origin city with `site`. Without a database,
or when it is unreachable, the response is a `503` with code `history_unavailable`.

```sh
curl -s 'http://localhost:8082/forecast/volume?weeks=8' \
  -H 'X-Internal-Token: dev-internal-token-change-me'
```

### POST /forecast/volume?weeks=8

The same forecast from a series the caller supplies:

```sh
curl -s 'http://localhost:8082/forecast/volume?weeks=2' \
  -H 'X-Internal-Token: dev-internal-token-change-me' \
  -H 'Content-Type: application/json' \
  -d '{"series":[{"week_start":"2026-05-04","count":41},{"week_start":"2026-05-11","count":45},
                 {"week_start":"2026-05-18","count":39},{"week_start":"2026-05-25","count":48}]}'
```

```json
{
  "weeks": [
    {"week_start": "2026-06-01", "point": 47.0, "low": 38.21, "high": 55.79},
    {"week_start": "2026-06-08", "point": 48.5, "low": 35.88, "high": 61.12}
  ],
  "method": "linear_trend",
  "history_weeks": 4,
  "source": "request"
}
```

Dates are aligned to the Monday of their week, counts in the same week are summed and
missing weeks count as zero. Fewer than 3 weeks is a `422` (`insufficient_history`).

### GET /healthz and GET /metrics

`/healthz` returns `{"status":"ok","model_loaded":true,"model_version":"..."}`.
`/metrics` is Prometheus text: `bowline_analytics_http_requests_total`,
`bowline_analytics_http_request_duration_seconds`, `bowline_analytics_scores_served_total`,
`bowline_analytics_forecasts_served_total`, `bowline_analytics_model_loaded` and the
`bowline_analytics_model_info` labels.

## The delay-risk model

There is no delay history in a fresh installation, so the model is trained on synthetic
shipments (`analytics/synthetic.py`) whose delay probability follows a documented latent
structure: sea freight and customs clearance raise the risk, air is the most reliable
mode, a carrier with a high on-time rate lowers it, a lead time that is short for the mode
raises it, hazardous cargo and long distances add a little, and seasonality peaks in
December. Labels are sampled from that probability after Gaussian noise on the logit
scale, and a share of `distance_km` and `carrier_on_time_rate` values are blanked as they
would be at booking time. The generator is seeded, so the same call always produces the
same rows.

`python -m analytics.train` then:

1. builds a scikit-learn `Pipeline`: a `ColumnTransformer` that one-hot encodes `mode`
   and passes the eight numeric features through, then a
   `HistGradientBoostingClassifier` (NaN-aware, so missing optional fields need no
   imputation);
2. splits the rows three ways (70% train, 15% calibration, 15% holdout, stratified);
3. fits the pipeline on the train split and calibrates its probabilities on the
   calibration split with `CalibratedClassifierCV` (isotonic for a full run, sigmoid for
   `--quick`, where isotonic would overfit);
4. reports AUC and Brier score on the untouched holdout, before and after calibration;
5. saves `delay_risk.joblib` (model, feature list, explanation baselines, metadata) and
   `delay_risk.json` (version, `trained_at`, row counts, `auc`, `brier`, calibration
   method, feature list).

`--quick` trains on 5,000 rows in a few seconds and is what CI runs; the full run uses
50,000 rows and is what the Docker image builds. The synthetic holdout is not a claim
about real-world accuracy: it shows the pipeline learns the structure it is given
(AUC around 0.8 on the quick run, higher on the full run) and that the probabilities are
calibrated. Once the `shipments` table carries `delivered_at` versus `eta` for enough
rows, the same pipeline can be retrained on real outcomes without changing the service.

## The volume forecast

For at least 16 weeks of history (`analytics/forecast.py`):

1. **Seasonal naive plus linear trend.** A least-squares line through the weekly counts,
   plus the detrended value observed 52 weeks earlier when a full season exists.
2. **Ridge on lag features.** A `Ridge` regression (after standardisation) on the counts
   at lags 1, 2, 4 and 8 weeks and the week-of-year as sine and cosine, applied
   recursively over the horizon.
3. The point forecast is the mean of the two components.
4. The 80% interval comes from the empirical 10th and 90th percentiles of one-step
   residuals collected by a rolling-origin backtest over the most recent 26 origins,
   widened with the square root of the horizon and never narrower than Poisson sampling
   noise for count data.

`method` in the response says what ran: `seasonal_naive_trend_ridge_blend` with a full
season of history, `trend_ridge_blend` with 16 to 55 weeks, and `linear_trend` (the
fallback) with 3 to 15 weeks.

## Tests and checks

```sh
ruff check . && ruff format --check .
python -m analytics.train --quick
python -m pytest -q
```

35 tests, and they need neither a database nor the network: the model is trained once per
session from the synthetic generator, and the database history source is replaced by a
fake. They cover the token guard and problem documents, generator determinism and label
rate, holdout AUC above 0.70 in `--quick` mode, the scoring contract (probability, band,
three drivers, derived fields, the clamp on saturated probabilities, monotonic sanity
between a risky sea booking and a reliable air booking), and the forecast (requested
horizon, `low <= point <= high`, trend fallback, rejection of a one-week series, database
and no-database paths).

## Docker

```sh
docker build -t bowline/analytics:dev analytics
docker run --rm -p 8082:8082 -e INTERNAL_SERVICE_TOKEN=dev-internal-token-change-me bowline/analytics:dev
```

The image is `python:3.12-slim`, runs as a non-root user, trains the model during the
build so it ships with one, and has a `HEALTHCHECK` against `/healthz`.
