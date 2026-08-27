"""Where the weekly shipment history for the volume forecast comes from.

When `ANALYTICS_DATABASE_URL` is set, weekly counts are read from `shipments.created_at`
through the read-only role, with a one second connect timeout so an unreachable database
costs the caller at most about a second. Without a database the GET endpoint reports that no
history is available and callers POST their own series instead.
"""

from __future__ import annotations

import logging
from typing import Protocol

import psycopg

from analytics.config import Settings
from analytics.forecast import WeekCount

log = logging.getLogger("analytics.history")

CONNECT_TIMEOUT_SECONDS = 1
STATEMENT_TIMEOUT_MS = 3000

WEEKLY_COUNTS_SQL = """
select date_trunc('week', created_at at time zone 'UTC')::date as week_start,
       count(*)::bigint as shipments
from shipments
where %(site)s::text is null
   or lower(origin ->> 'city') = lower(%(site)s::text)
group by 1
order by 1
"""


class HistoryUnavailableError(RuntimeError):
    """No history source can answer right now."""


class HistorySource(Protocol):
    name: str

    def fetch(self, site: str | None) -> list[WeekCount]: ...


class NoHistorySource:
    """Used when no database is configured."""

    name = "none"

    def fetch(self, site: str | None) -> list[WeekCount]:
        raise HistoryUnavailableError("ANALYTICS_DATABASE_URL is not configured")


class DatabaseHistorySource:
    """Weekly shipment counts from PostgreSQL."""

    name = "database"

    def __init__(self, dsn: str) -> None:
        self._dsn = dsn

    def fetch(self, site: str | None) -> list[WeekCount]:
        try:
            with psycopg.connect(
                self._dsn,
                connect_timeout=CONNECT_TIMEOUT_SECONDS,
                options=f"-c statement_timeout={STATEMENT_TIMEOUT_MS}",
                application_name="bowline-analytics",
                autocommit=True,
            ) as connection:
                connection.read_only = True
                with connection.cursor() as cursor:
                    cursor.execute(WEEKLY_COUNTS_SQL, {"site": site})
                    rows = cursor.fetchall()
        except psycopg.OperationalError as exc:
            log.warning("shipment history database not reachable", extra={"error": str(exc)})
            raise HistoryUnavailableError("the shipment history database is not reachable") from exc
        except psycopg.Error as exc:
            log.warning("shipment history query failed", extra={"error": str(exc)})
            raise HistoryUnavailableError("the shipment history query failed") from exc
        return [WeekCount(week_start=row[0], count=int(row[1])) for row in rows]


def build_history_source(settings: Settings) -> HistorySource:
    if settings.database_url:
        return DatabaseHistorySource(settings.database_url)
    log.info("no ANALYTICS_DATABASE_URL; GET /forecast/volume will report history_unavailable")
    return NoHistorySource()
