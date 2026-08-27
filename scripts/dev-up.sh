#!/usr/bin/env bash
# Start the whole Bowline stack locally, in the background.
#
#   ./scripts/dev-up.sh          start everything
#   ./scripts/dev-down.sh        stop the application services again
#
# Logs land in .dev-logs/ and each service writes its PID to .dev-logs/*.pid.
set -uo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
LOGS="$ROOT/.dev-logs"
mkdir -p "$LOGS"
export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:$PATH"

green=$'\033[32m'; red=$'\033[31m'; dim=$'\033[2m'; off=$'\033[0m'
[ -t 1 ] || { green=""; red=""; dim=""; off=""; }
say()  { printf '  %s\n' "$1"; }
ok()   { printf '  %s%s%s %s\n' "$green" "ok  " "$off" "$1"; }
bad()  { printf '  %s%s%s %s\n' "$red"   "FAIL" "$off" "$1"; }

# ---- shared configuration ---------------------------------------------------
export DATABASE_URL='postgres://bowline_app:bowline_app_dev@localhost:5432/bowline'
export DATABASE_URL_NOTIFY='postgres://bowline_notify:bowline_notify_dev@localhost:5432/bowline'
export REDIS_URL='redis://localhost:6379/0'
export JWT_SECRET='dev-only-change-me-0123456789abcdef0123456789abcdef'
export INTERNAL_SERVICE_TOKEN='dev-internal-token-change-me'
export API_BIND='127.0.0.1:8080'
export API_CORS_ORIGINS='http://localhost:3000'
export S3_ENDPOINT='http://localhost:9000'
export S3_REGION='us-east-1'
export S3_ACCESS_KEY_ID='minioadmin'
export S3_SECRET_ACCESS_KEY='minioadmin'
export S3_BUCKET_DOCUMENTS='bowline-documents'
export S3_BUCKET_PDFS='bowline-pdfs'
export S3_FORCE_PATH_STYLE=1
export BILLING_URL='http://localhost:8081'
export ANALYTICS_URL='http://localhost:8082'
export SMTP_HOST=localhost
export SMTP_PORT=1025
export MAIL_FROM='Bowline <no-reply@bowline.example>'
export NOTIFY_METRICS_BIND='127.0.0.1:9101'
export LOG_FORMAT=pretty
export RUST_LOG='info,tower_http=warn,sqlx=warn'

start() { # start NAME COMMAND...
  local name="$1"; shift
  if [ -f "$LOGS/$name.pid" ] && kill -0 "$(cat "$LOGS/$name.pid")" 2>/dev/null; then
    say "$name already running (pid $(cat "$LOGS/$name.pid"))"; return
  fi
  "$@" > "$LOGS/$name.log" 2>&1 &
  echo $! > "$LOGS/$name.pid"
  say "$name starting (pid $!), log: .dev-logs/$name.log"
}

wait_for() { # wait_for NAME URL SECONDS
  local name="$1" url="$2" secs="${3:-60}" i
  for i in $(seq 1 "$secs"); do
    if [ "$(curl -s -o /dev/null -w '%{http_code}' "$url" 2>/dev/null)" = "200" ]; then
      ok "$name  $url"; return 0
    fi
    sleep 1
  done
  bad "$name did not come up at $url (see .dev-logs/$name.log)"
  return 1
}

echo
echo "1. Infrastructure (Postgres, Redis, Mailpit, MinIO)"
docker compose up -d postgres redis mailpit minio minio-init >/dev/null 2>&1
for i in $(seq 1 40); do
  docker compose exec -T postgres pg_isready -U postgres >/dev/null 2>&1 && break
  sleep 1
done
ok "postgres, redis, mailpit, minio"

echo
echo "2. Building the API (first run takes a few minutes)"
( cd api && cargo build --quiet --bin bowline-api --bin seed ) || { bad "api build failed"; exit 1; }
ok "built"

echo
echo "3. Seeding the demo company if it is not there yet"
( cd api && SEED_PASSWORD='Bowline!2026' SEED_SKIP_PASSWORD_CHANGE=1 SEED_RANDOM_SEED=42 \
    cargo run --quiet --bin seed ) 2>&1 | tail -3

echo
echo "4. Services"
start api       "$ROOT/api/target/debug/bowline-api"
start billing   env BILLING_PDF_OUTPUT=s3 BILLING_BIND_PORT=8081 \
                    BILLING_DATABASE_URL='jdbc:postgresql://localhost:5432/bowline' \
                    BILLING_DATABASE_USER=bowline_ro BILLING_DATABASE_PASSWORD=bowline_ro_dev \
                    "$ROOT/billing/mvnw" -q -f "$ROOT/billing/pom.xml" spring-boot:run
# analytics is a package, so it has to be started from its own directory
start analytics env ANALYTICS_BIND_PORT=8082 sh -c "cd '$ROOT/analytics' && exec .venv/bin/python -m analytics.main"
start notify    "$ROOT/tools/bin/notify"
start web       npm --prefix "$ROOT/web" run dev

echo
echo "5. Waiting for them to answer"
wait_for api       http://localhost:8080/healthz 90
wait_for billing   http://localhost:8081/healthz 120
wait_for analytics http://localhost:8082/healthz 60
wait_for web       http://localhost:3000/login   90

cat <<BANNER

  Open http://localhost:3000 and sign in.

  Password for every seeded account:  Bowline!2026

  ceo@bowline.example            see everything, announce to the company
  cfo@bowline.example            finance, payroll, close a period
  accountant@bowline.example     invoices, payments, the ledger
  hr.admin@bowline.example       employees, leave, documents
  dispatcher@bowline.example     shipments, fleet, work orders
  supervisor.dock@bowline.example  approve leave for their team
  support.agent@bowline.example  the service desk queue
  driver@bowline.example         a phone-shaped task list
  dock.worker@bowline.example    the narrowest view in the company

  Also running:
  http://localhost:8080/docs     API reference (every endpoint, try it live)
  http://localhost:8025          Mailpit, every email the platform sends
  http://localhost:9001          MinIO console (minioadmin / minioadmin)

  Stop it all with:  ./scripts/dev-down.sh
  Follow a log with: tail -f .dev-logs/api.log

BANNER
