"""Service configuration.

Every value comes from the environment. The variable names match `.env.example` at the
repository root; nothing is read from files at runtime.
"""

from __future__ import annotations

from pathlib import Path
from typing import Literal

from pydantic import Field, field_validator
from pydantic_settings import BaseSettings, SettingsConfigDict

DEFAULT_MODEL_PATH = Path("./models/delay_risk.joblib")


class Settings(BaseSettings):
    """Runtime settings for the analytics service.

    Construct with the environment names when building settings in code, for example
    `Settings(INTERNAL_SERVICE_TOKEN="...", ANALYTICS_MODEL_PATH="...")`.
    """

    model_config = SettingsConfigDict(
        extra="ignore",
        case_sensitive=False,
        protected_namespaces=(),
    )

    bind_port: int = Field(8082, validation_alias="ANALYTICS_BIND_PORT", ge=1, le=65535)
    database_url: str | None = Field(None, validation_alias="ANALYTICS_DATABASE_URL")
    model_path: Path = Field(DEFAULT_MODEL_PATH, validation_alias="ANALYTICS_MODEL_PATH")
    internal_service_token: str = Field("", validation_alias="INTERNAL_SERVICE_TOKEN")
    log_format: Literal["json", "pretty"] = Field("json", validation_alias="LOG_FORMAT")
    log_level: str = Field("INFO", validation_alias="ANALYTICS_LOG_LEVEL")

    @field_validator("database_url", mode="before")
    @classmethod
    def _blank_is_none(cls, value: object) -> object:
        if isinstance(value, str) and not value.strip():
            return None
        return value

    @field_validator("log_level")
    @classmethod
    def _upper_level(cls, value: str) -> str:
        level = value.strip().upper()
        if level not in {"DEBUG", "INFO", "WARNING", "ERROR", "CRITICAL"}:
            raise ValueError("ANALYTICS_LOG_LEVEL must be DEBUG, INFO, WARNING, ERROR or CRITICAL")
        return level

    @property
    def token_configured(self) -> bool:
        return bool(self.internal_service_token.strip())
