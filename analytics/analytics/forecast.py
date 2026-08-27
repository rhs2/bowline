"""Weekly shipment volume forecast.

Method (`seasonal_naive_trend_ridge_blend`), for at least `MIN_FULL_WEEKS` of history:

1. Component A, seasonal naive plus linear trend: a least-squares line through the weekly
   counts, plus the detrended value observed 52 weeks earlier when a full season exists
   (without one the component reduces to the trend line, and the method is reported as
   `trend_ridge_blend`).
2. Component B, a Ridge regression on lag features (lags 1, 2, 4 and 8 weeks) and the
   week-of-year encoded as sine and cosine, applied recursively over the horizon.
3. The point forecast is the mean of A and B.
4. The 80% interval comes from empirical quantiles (10th and 90th) of one-step residuals
   collected by a rolling-origin backtest over the most recent origins, widened with the
   square root of the horizon and never narrower than Poisson sampling noise.

With fewer than `MIN_FULL_WEEKS` but at least `MIN_WEEKS` points the service falls back to
the linear trend alone (`linear_trend`). Shorter histories are rejected.
"""

from __future__ import annotations

import math
from dataclasses import dataclass
from datetime import date, timedelta

import numpy as np
from sklearn.linear_model import Ridge
from sklearn.pipeline import make_pipeline
from sklearn.preprocessing import StandardScaler

MIN_WEEKS = 3
MIN_FULL_WEEKS = 16
MAX_SPAN_WEEKS = 520
SEASON_WEEKS = 52
LAGS: tuple[int, ...] = (1, 2, 4, 8)
MAX_LAG = max(LAGS)
WEEKS_PER_YEAR = 52.1775
INTERVAL_QUANTILES = (0.10, 0.90)
Z_80 = 1.2816  # one-sided 90% normal quantile, used for the Poisson floor
MIN_BACKTEST_ORIGIN = MAX_LAG + 4
BACKTEST_MAX_ORIGINS = 26
RIDGE_ALPHA = 1.0

METHOD_TREND = "linear_trend"
METHOD_BLEND = "trend_ridge_blend"
METHOD_SEASONAL_BLEND = "seasonal_naive_trend_ridge_blend"


class SeriesError(ValueError):
    """The provided history cannot be used."""


class InsufficientHistoryError(SeriesError):
    """Too few weekly points to forecast."""


@dataclass(frozen=True)
class WeekCount:
    week_start: date
    count: int


@dataclass(frozen=True)
class ForecastWeek:
    week_start: date
    point: float
    low: float
    high: float


@dataclass(frozen=True)
class VolumeForecast:
    weeks: list[ForecastWeek]
    method: str
    history_weeks: int


def week_monday(day: date) -> date:
    return day - timedelta(days=day.weekday())


def normalise_series(series: list[WeekCount]) -> tuple[list[date], np.ndarray]:
    """Align to Monday-based weeks, sum duplicates, fill gaps with zero, sort ascending."""
    if not series:
        raise InsufficientHistoryError("no weekly history was provided")
    totals: dict[date, int] = {}
    for item in series:
        if item.count < 0:
            raise SeriesError("weekly counts cannot be negative")
        monday = week_monday(item.week_start)
        totals[monday] = totals.get(monday, 0) + int(item.count)
    first, last = min(totals), max(totals)
    span = (last - first).days // 7 + 1
    if span > MAX_SPAN_WEEKS:
        raise SeriesError(f"history spans {span} weeks; at most {MAX_SPAN_WEEKS} are supported")
    weeks = [first + timedelta(weeks=i) for i in range(span)]
    values = np.array([float(totals.get(week, 0)) for week in weeks], dtype=float)
    return weeks, values


def forecast_volume(series: list[WeekCount], horizon: int) -> VolumeForecast:
    """Forecast `horizon` weeks beyond the last week of `series`."""
    if horizon < 1:
        raise ValueError("horizon must be at least one week")
    weeks, y = normalise_series(series)
    n = len(y)
    if n < MIN_WEEKS:
        raise InsufficientHistoryError(
            f"at least {MIN_WEEKS} weeks of history are required, got {n}"
        )

    if n < MIN_FULL_WEEKS:
        points = _trend_forecast(y, horizon)
        residuals = _trend_residuals(y)
        method = METHOD_TREND
    else:
        points, seasonal = _blend_forecast(y, weeks[0], horizon)
        residuals = _backtest_residuals(y, weeks[0])
        method = METHOD_SEASONAL_BLEND if seasonal else METHOD_BLEND

    lows, highs = _interval(points, residuals)
    future = [weeks[-1] + timedelta(weeks=h) for h in range(1, horizon + 1)]
    return VolumeForecast(
        weeks=[
            ForecastWeek(
                week_start=week,
                point=round(float(point), 2),
                low=round(float(low), 2),
                high=round(float(high), 2),
            )
            for week, point, low, high in zip(future, points, lows, highs, strict=True)
        ],
        method=method,
        history_weeks=n,
    )


def _fit_trend(y: np.ndarray) -> tuple[float, float]:
    """Least-squares line through the history; returns (intercept, slope)."""
    t = np.arange(len(y), dtype=float)
    slope, intercept = np.polyfit(t, y, 1)
    return float(intercept), float(slope)


def _trend_forecast(y: np.ndarray, horizon: int) -> np.ndarray:
    intercept, slope = _fit_trend(y)
    n = len(y)
    steps = np.arange(n, n + horizon, dtype=float)
    return np.maximum(0.0, intercept + slope * steps)


def _trend_residuals(y: np.ndarray) -> np.ndarray:
    intercept, slope = _fit_trend(y)
    return y - (intercept + slope * np.arange(len(y), dtype=float))


def _week_of_year_terms(first_week: date, t: int) -> tuple[float, float]:
    week = (first_week + timedelta(weeks=t)).isocalendar().week
    angle = 2.0 * math.pi * week / WEEKS_PER_YEAR
    return math.sin(angle), math.cos(angle)


def _lag_row(values: np.ndarray, t: int, first_week: date) -> list[float]:
    sin_term, cos_term = _week_of_year_terms(first_week, t)
    return [float(values[t - lag]) for lag in LAGS] + [sin_term, cos_term]


def _blend_forecast(y: np.ndarray, first_week: date, horizon: int) -> tuple[np.ndarray, bool]:
    """Point forecasts from the blend of components A and B; also whether A used a season."""
    n = len(y)
    intercept, slope = _fit_trend(y)
    detrended = y - (intercept + slope * np.arange(n, dtype=float))
    seasonal = n >= SEASON_WEEKS + 4

    rows = [_lag_row(y, t, first_week) for t in range(MAX_LAG, n)]
    targets = y[MAX_LAG:n]
    ridge = make_pipeline(StandardScaler(), Ridge(alpha=RIDGE_ALPHA)).fit(rows, targets)

    extended = list(y)
    points = np.empty(horizon, dtype=float)
    for h in range(1, horizon + 1):
        t = n - 1 + h
        component_a = intercept + slope * t
        if seasonal and 0 <= t - SEASON_WEEKS < n:
            component_a += float(detrended[t - SEASON_WEEKS])
        component_b = float(
            ridge.predict([_lag_row(np.asarray(extended, dtype=float), t, first_week)])[0]
        )
        point = max(0.0, 0.5 * component_a + 0.5 * component_b)
        points[h - 1] = point
        extended.append(point)
    return points, seasonal


def _backtest_residuals(y: np.ndarray, first_week: date) -> np.ndarray:
    """One-step-ahead residuals of the blend over the most recent forecast origins."""
    n = len(y)
    first_origin = max(MIN_BACKTEST_ORIGIN, n - BACKTEST_MAX_ORIGINS)
    residuals = [
        float(y[origin]) - float(_blend_forecast(y[:origin], first_week, 1)[0][0])
        for origin in range(first_origin, n)
    ]
    return np.asarray(residuals, dtype=float)


def _interval(points: np.ndarray, residuals: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """80% interval: empirical residual quantiles, widened with sqrt(horizon), Poisson floor."""
    if len(residuals) >= MIN_WEEKS and float(np.ptp(residuals)) > 1e-9:
        q_low = min(0.0, float(np.quantile(residuals, INTERVAL_QUANTILES[0])))
        q_high = max(0.0, float(np.quantile(residuals, INTERVAL_QUANTILES[1])))
    else:
        q_low = q_high = 0.0

    lows = np.empty_like(points)
    highs = np.empty_like(points)
    for index, point in enumerate(points):
        scale = math.sqrt(index + 1)
        poisson_half = Z_80 * math.sqrt(max(float(point), 1.0))
        width_low = max(-q_low, poisson_half) * scale
        width_high = max(q_high, poisson_half) * scale
        lows[index] = max(0.0, float(point) - width_low)
        highs[index] = float(point) + width_high
    return lows, highs
