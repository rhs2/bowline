"""Request and response models."""

from __future__ import annotations

from datetime import UTC, date, datetime
from enum import StrEnum
from typing import Literal

from pydantic import BaseModel, ConfigDict, Field, field_validator, model_validator

from analytics.features import MAX_LEAD_DAYS


class Mode(StrEnum):
    sea = "sea"
    air = "air"
    road = "road"
    rail = "rail"


def _coerce_date(value: object) -> object:
    """Accept `YYYY-MM-DD` or an RFC 3339 timestamp (taken in UTC) for a date field."""
    if isinstance(value, datetime):
        return (value.astimezone(UTC) if value.tzinfo else value).date()
    if isinstance(value, str) and "T" in value:
        parsed = datetime.fromisoformat(value)
        return (parsed.astimezone(UTC) if parsed.tzinfo else parsed).date()
    return value


class ScoreRequest(BaseModel):
    """Body of POST /score/delay-risk (the shape the API sends when a shipment is booked)."""

    model_config = ConfigDict(extra="ignore", json_schema_extra={"example": {}})

    mode: Mode
    weight_kg: float = Field(ge=0, le=10_000_000)
    pieces: int = Field(ge=1, le=1_000_000)
    hazardous: bool
    distance_km: float | None = Field(default=None, ge=0, le=50_000)
    carrier_on_time_rate: float | None = Field(default=None, ge=0, le=1)
    etd: date
    eta: date
    customs: bool | None = Field(
        default=None,
        description="Override the customs flag; by default sea and air shipments clear customs.",
    )

    @field_validator("etd", "eta", mode="before")
    @classmethod
    def _dates(cls, value: object) -> object:
        return _coerce_date(value)

    @model_validator(mode="after")
    def _window(self) -> ScoreRequest:
        if self.eta < self.etd:
            raise ValueError("eta must be on or after etd")
        if (self.eta - self.etd).days > MAX_LEAD_DAYS:
            raise ValueError(f"lead time must be at most {MAX_LEAD_DAYS} days")
        return self


class DriverOut(BaseModel):
    feature: str
    direction: Literal["increases", "decreases", "neutral"]
    delta: float = Field(description="Change in risk versus the feature's baseline value")


class DerivedOut(BaseModel):
    lead_days: int
    month: int
    customs: bool


class ScoreResponse(BaseModel):
    risk: float = Field(ge=0, le=1)
    band: Literal["low", "medium", "high"]
    model_version: str
    drivers: list[DriverOut]
    derived: DerivedOut


class WeekCountIn(BaseModel):
    week_start: date
    count: int = Field(ge=0)

    @field_validator("week_start", mode="before")
    @classmethod
    def _dates(cls, value: object) -> object:
        return _coerce_date(value)


class VolumeSeriesRequest(BaseModel):
    """Body of POST /forecast/volume: the caller's own weekly history."""

    model_config = ConfigDict(extra="ignore")

    series: list[WeekCountIn] = Field(min_length=1, max_length=1040)


class ForecastWeekOut(BaseModel):
    week_start: date
    point: float
    low: float
    high: float


class VolumeForecastResponse(BaseModel):
    weeks: list[ForecastWeekOut]
    method: str
    history_weeks: int
    source: Literal["database", "request"]


class HealthResponse(BaseModel):
    status: Literal["ok"]
    model_loaded: bool
    model_version: str | None = None
