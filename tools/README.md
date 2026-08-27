# Bowline tools

Two Go programs: `notify`, the worker that turns outbox rows into email, and
`bowctl`, the operator command line for the API.

```
cmd/notify/          the worker binary and its configuration
cmd/bowctl/          the CLI binary
internal/outbox/     Store and Sender interfaces, the worker loop, Postgres store, metrics
internal/mail/       SMTP sender and RFC 5322 message building
internal/apiclient/  typed API client with transparent token refresh
internal/bowctl/     argument parsing and the command implementations
internal/creds/      the session file ($HOME/.config/bowline/credentials.json, mode 0600)
internal/env/        environment reader with defaults, validation and error collection
```

## notify

Every message, announcement and ticket reply the API writes also inserts a row in
`notifications`, in the same transaction as the business change. That is the
transactional outbox: if the API commits, the email is guaranteed to be queued, and
if the mail provider is down nothing is lost.

The worker polls with `SELECT ... FOR UPDATE SKIP LOCKED`, so several replicas can
run at once without ever claiming the same row.

| Behaviour        | Detail                                                                                     |
|------------------|---------------------------------------------------------------------------------------------|
| Claim            | up to `NOTIFY_BATCH_SIZE` rows (default 50) whose `next_attempt_at` has passed, marked `sending` |
| Poll interval    | `NOTIFY_POLL_INTERVAL_MS`, default 2000                                                     |
| Retry            | `30s * 2^attempts` with jitter, capped at one hour                                          |
| Park             | after `NOTIFY_MAX_ATTEMPTS` (default 8) the row becomes `failed` with `last_error` kept      |
| Permanent errors | a 5xx SMTP reply or an unbuildable message is parked immediately, without burning retries    |
| Crash recovery   | rows left in `sending` for more than 10 minutes are reclaimed on a later poll               |
| Shutdown         | SIGTERM and SIGINT stop the poll loop and let the batch in flight finish                     |

Delivery goes to Mailpit locally and Amazon SES in production; the only difference
is the SMTP variables. Each message carries `Message-ID`, `Date` and
`X-Bowline-Notification-Id` so a delivery can be traced back to its row.

```bash
DATABASE_URL_NOTIFY=postgres://bowline_notify:bowline_notify_dev@localhost:5432/bowline \
SMTP_HOST=localhost SMTP_PORT=1025 \
MAIL_FROM="Bowline <no-reply@bowline.example>" \
go run ./cmd/notify
```

The worker connects as `bowline_notify`, a role that may only `SELECT` and `UPDATE`
the `notifications` table. It cannot read employees, invoices or anything else.

**Metrics** on `NOTIFY_METRICS_BIND` (default `0.0.0.0:9101`), plus `/healthz`:
`bowline_notify_sent_total`, `_failed_total`, `_retried_total`, `_poll_errors_total`,
`_batch_size`, `_send_duration_seconds`, `_outbox_depth`.

## bowctl

```bash
go run ./cmd/bowctl health
go run ./cmd/bowctl login --email ceo@bowline.example --password-stdin
go run ./cmd/bowctl whoami
go run ./cmd/bowctl broadcast --scope company --subject "Peak season" --body-file notice.txt
go run ./cmd/bowctl ticket --category it --priority high --subject "Scanner offline" --body "Bay 3"
go run ./cmd/bowctl employees --q ada --department <uuid> --limit 50
go run ./cmd/bowctl outbox depth
```

Sessions are stored in `$HOME/.config/bowline/credentials.json` with mode 0600. The
client refreshes an expired access token once, transparently, and persists the
rotated refresh token. RFC 7807 problem documents from the API are rendered as
readable errors including the field list and the request id. Exit codes: 0 success,
1 the command failed, 2 a usage error. `bowctl help` prints the full flag reference.

## Tests

```bash
go vet ./... && go test ./...
```

58 tests, no database and no network required. The worker is tested against
in-memory `Store` and `Sender` fakes (happy path, retry with backoff, parking after
the last attempt, reclaiming stale `sending` rows, shutdown mid-batch); the SMTP
sender against a fake SMTP server (including STARTTLS refusal, permanent 5xx
replies and context deadlines); the API client against `httptest` servers
(refresh on 401, problem parsing, pagination).

## Docker

```bash
docker build --target notify -t bowline/notify:dev .
docker build --target bowctl -t bowline/bowctl:dev .
```

Both final images are non-root and carry only the static binary and CA certificates.
