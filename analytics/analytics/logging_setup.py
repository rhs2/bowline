"""Structured logging.

`json` (the production default) writes one JSON object per line to stdout so the ECS log
driver and CloudWatch can index the fields. `pretty` is for a terminal. Extra fields passed
through `logger.info("...", extra={...})` are emitted as top-level keys.
"""

from __future__ import annotations

import json
import logging
import sys
from datetime import UTC, datetime
from typing import Any, Literal

# Attributes that every LogRecord carries; everything else is treated as a structured field.
_RESERVED = frozenset(
    {
        "name",
        "msg",
        "args",
        "levelname",
        "levelno",
        "pathname",
        "filename",
        "module",
        "exc_info",
        "exc_text",
        "stack_info",
        "lineno",
        "funcName",
        "created",
        "msecs",
        "relativeCreated",
        "thread",
        "threadName",
        "processName",
        "process",
        "message",
        "taskName",
        "asctime",
        "color_message",  # uvicorn's terminal-only duplicate of msg
    }
)


def _structured_fields(record: logging.LogRecord) -> dict[str, Any]:
    return {
        key: value
        for key, value in record.__dict__.items()
        if key not in _RESERVED and not key.startswith("_")
    }


class JsonFormatter(logging.Formatter):
    """One JSON document per line."""

    def format(self, record: logging.LogRecord) -> str:
        payload: dict[str, Any] = {
            "ts": datetime.fromtimestamp(record.created, tz=UTC).isoformat(timespec="milliseconds"),
            "level": record.levelname,
            "logger": record.name,
            "msg": record.getMessage(),
        }
        payload.update(_structured_fields(record))
        if record.exc_info:
            payload["exc"] = self.formatException(record.exc_info)
        return json.dumps(payload, default=str, separators=(",", ":"))


class PrettyFormatter(logging.Formatter):
    """Human-readable line with the structured fields appended as key=value pairs."""

    def format(self, record: logging.LogRecord) -> str:
        line = super().format(record)
        fields = _structured_fields(record)
        if fields:
            line = f"{line} " + " ".join(f"{key}={value}" for key, value in fields.items())
        return line


def configure_logging(log_format: Literal["json", "pretty"] = "json", level: str = "INFO") -> None:
    """Install a single stdout handler on the root logger and route uvicorn through it."""
    handler = logging.StreamHandler(sys.stdout)
    if log_format == "json":
        handler.setFormatter(JsonFormatter())
    else:
        handler.setFormatter(PrettyFormatter("%(asctime)s %(levelname)-7s %(name)s: %(message)s"))

    root = logging.getLogger()
    root.handlers = [handler]
    root.setLevel(level.upper())

    for name in ("uvicorn", "uvicorn.error", "uvicorn.access"):
        uvicorn_logger = logging.getLogger(name)
        uvicorn_logger.handlers = []
        uvicorn_logger.propagate = True
    # The request middleware writes its own access log with request ids and latency.
    logging.getLogger("uvicorn.access").setLevel(logging.WARNING)
