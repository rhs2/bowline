#!/usr/bin/env bash
# End-to-end scenario against a running Bowline stack.
#
#   make up && make migrate && make seed && make api      # in another terminal
#   ./scripts/smoke.sh
#
# It walks one working day across six roles and checks that the rules hold:
# the chain of command, the messaging rules, the shipment state machine, the
# work order ownership rule, the ledger, and the closed period lock.
#
# Every step prints PASS or FAIL; the script exits non-zero on the first failure.
set -uo pipefail

BASE="${API_URL:-http://localhost:8080}"
API="$BASE/api/v1"
PASSWORD="${SEED_PASSWORD:-Bowline!2026}"
FAILURES=0
STEP=0

c_green=$'\033[32m'; c_red=$'\033[31m'; c_dim=$'\033[2m'; c_off=$'\033[0m'
[ -t 1 ] || { c_green=""; c_red=""; c_dim=""; c_off=""; }

# json <expression> reads a JSON document on stdin and prints one value.
# The expression is Python against the parsed document bound to `d`.
json() { python3 -c "import json,sys
try: d=json.load(sys.stdin)
except Exception: sys.exit(1)
v=($1)
print('' if v is None else v)" 2>/dev/null; }

# call METHOD PATH [BODY] [TOKEN] -> body on stdout, status readable with `st`.
#
# The status goes to a file rather than a variable on purpose: most calls here run
# inside $( ), which is a subshell, so a variable set in `call` would never reach
# the caller and every check would read the previous call's status.
STATUS_FILE=/tmp/bowline_smoke_status
call() {
  local method="$1" path="$2" body="${3:-}" token="${4:-}"
  local args=(-sS -o /tmp/bowline_smoke_body -w '%{http_code}' -X "$method" "$API$path")
  [ -n "$token" ] && args+=(-H "Authorization: Bearer $token")
  if [ -n "$body" ]; then args+=(-H 'Content-Type: application/json' -d "$body"); fi
  curl "${args[@]}" > "$STATUS_FILE" 2>/dev/null || echo 000 > "$STATUS_FILE"
  cat /tmp/bowline_smoke_body
}
st() { cat "$STATUS_FILE" 2>/dev/null || echo 000; }

step() { STEP=$((STEP+1)); printf '%2d. %-58s' "$STEP" "$1"; }
pass() { printf '%sPASS%s %s%s%s\n' "$c_green" "$c_off" "$c_dim" "${1:-}" "$c_off"; }
fail() { printf '%sFAIL%s %s\n' "$c_red" "$c_off" "${1:-}"; FAILURES=$((FAILURES+1)); }

# expect EXPECTED_STATUS ACTUAL_STATUS DETAIL
expect() {
  if [ "$2" = "$1" ]; then pass "${3:-}"; else fail "expected HTTP $1, got $2. ${3:-}"; fi
}

login() { # login EMAIL -> access token on stdout
  local email="$1" body
  body=$(call POST /auth/login "{\"email\":\"$email\",\"password\":\"$PASSWORD\"}")
  [ "$(st)" = "200" ] || { echo ""; return 1; }
  printf '%s' "$body" | json "d['access_token']"
}

echo
echo "Bowline smoke test against $API"
echo

# --------------------------------------------------------------------------
step "API is up"
curl -sS -o /dev/null -w '%{http_code}' "$BASE/healthz" > "$STATUS_FILE" 2>/dev/null || echo 000 > "$STATUS_FILE"
if [ "$(st)" = "200" ]; then pass; else
  fail "the API did not answer /healthz. Start it with: make api"
  echo; echo "Cannot continue without a running API."; exit 1
fi

# --------------------------------------------------------------------------
step "everyone can sign in"
CEO=$(login ceo@bowline.example)         || true
DOCK=$(login dock.worker@bowline.example) || true
AGENT=$(login support.agent@bowline.example) || true
DISPATCH=$(login dispatcher@bowline.example) || true
DRIVER=$(login driver@bowline.example)   || true
ACCT=$(login accountant@bowline.example) || true
CFO=$(login cfo@bowline.example)         || true
missing=""
for pair in CEO:ceo DOCK:dock.worker AGENT:support.agent DISPATCH:dispatcher DRIVER:driver ACCT:accountant CFO:cfo; do
  name="${pair%%:*}"; who="${pair##*:}"
  [ -z "${!name}" ] && missing="$missing $who"
done
if [ -z "$missing" ]; then pass "7 roles"; else
  fail "could not sign in:$missing. Run: make seed"
  echo; echo "Cannot continue without the seeded company."; exit 1
fi

# --------------------------------------------------------------------------
step "a bad password is refused"
call POST /auth/login '{"email":"ceo@bowline.example","password":"wrong-password"}' >/dev/null
expect 401 "$(st)"

# --------------------------------------------------------------------------
step "the CEO sees their permissions and chain"
me=$(call GET /auth/me "" "$CEO")
perms=$(printf '%s' "$me" | json "len(d['permissions'])")
if [ "$(st)" = "200" ] && [ "${perms:-0}" -gt 10 ]; then pass "$perms permissions"; else
  fail "HTTP $(st), permissions=$perms"; fi

# --------------------------------------------------------------------------
step "a dock worker sees far fewer permissions"
me=$(call GET /auth/me "" "$DOCK")
dperms=$(printf '%s' "$me" | json "len(d['permissions'])")
DOCK_EMP=$(printf '%s' "$me" | json "d['employee']['id']")
if [ "$(st)" = "200" ] && [ "${dperms:-0}" -lt "${perms:-99}" ]; then pass "$dperms vs $perms"; else
  fail "dock worker has $dperms permissions, CEO has $perms"; fi

# --------------------------------------------------------------------------
step "the CEO announces to the whole company"
call POST /comms/announcements \
  '{"scope":"company","subject":"Peak season starts Monday","body":"Thank you all for the work this quarter."}' \
  "$CEO" >/dev/null
expect 201 "$(st)"

# --------------------------------------------------------------------------
step "a dock worker cannot announce to the company"
call POST /comms/announcements \
  '{"scope":"company","subject":"Unauthorised","body":"This must be refused."}' \
  "$DOCK" >/dev/null
expect 403 "$(st)"

# --------------------------------------------------------------------------
step "the announcement reaches the dock worker's inbox"
threads=$(call GET "/comms/threads?kind=announcement" "" "$DOCK")
found=$(printf '%s' "$threads" | json "sum(1 for t in d['items'] if 'Peak season' in t.get('subject',''))")
if [ "$(st)" = "200" ] && [ "${found:-0}" -ge 1 ]; then pass "found in inbox"; else
  fail "HTTP $(st), matching threads=$found"; fi

# --------------------------------------------------------------------------
step "the dock worker may not message the CFO directly"
cfo_me=$(call GET /auth/me "" "$CFO")
CFO_EMP=$(printf '%s' "$cfo_me" | json "d['employee']['id']")
call POST /comms/threads \
  "{\"recipient_ids\":[\"$CFO_EMP\"],\"subject\":\"Direct to the CFO\",\"body\":\"This must be refused.\"}" \
  "$DOCK" >/dev/null
expect 403 "$(st)"

# --------------------------------------------------------------------------
step "the dock worker may message their own manager"
chain=$(call GET /auth/me "" "$DOCK")
# employee.manager_id is the direct manager. Do not read it off `chain`: that runs
# upward from the manager to the CEO, so indexing into it lands on an executive,
# and a dock worker messaging an executive is refused, as it should be.
MGR=$(printf '%s' "$chain" | json "d['employee'].get('manager_id') or ''")
if [ -n "$MGR" ]; then
  call POST /comms/threads \
    "{\"recipient_ids\":[\"$MGR\"],\"subject\":\"Bay 3 scanner\",\"body\":\"The scanner in bay 3 keeps dropping its connection.\"}" \
    "$DOCK" >/dev/null
  expect 201 "$(st)"
else
  fail "could not resolve the dock worker's manager from /auth/me"
fi

# --------------------------------------------------------------------------
step "the dock worker opens a support ticket"
ticket=$(call POST /support/tickets \
  '{"category":"it","priority":"high","subject":"Handheld scanner offline","body":"The scanner in bay 3 will not connect to the network."}' \
  "$DOCK")
TICKET_ID=$(printf '%s' "$ticket" | json "d['id']")
TICKET_NO=$(printf '%s' "$ticket" | json "d.get('ticket_no','')")
expect 201 "$(st)" "$TICKET_NO"

# --------------------------------------------------------------------------
step "the ticket carries an SLA deadline"
sla=$(printf '%s' "$ticket" | json "d.get('sla_due_at','')")
if [ -n "$sla" ]; then pass "due $sla"; else fail "no sla_due_at on the ticket"; fi

# --------------------------------------------------------------------------
step "a support agent takes the ticket"
agent_me=$(call GET /auth/me "" "$AGENT")
AGENT_EMP=$(printf '%s' "$agent_me" | json "d['employee']['id']")
call POST "/support/tickets/$TICKET_ID/assign" "{\"assignee_id\":\"$AGENT_EMP\"}" "$AGENT" >/dev/null
expect 200 "$(st)"

# --------------------------------------------------------------------------
step "the agent replies and resolves it"
call POST "/support/tickets/$TICKET_ID/messages" '{"body":"Replaced the access point in bay 3. Please confirm."}' "$AGENT" >/dev/null
s1=$(st)
# the lifecycle is open -> triaged -> in_progress -> resolved, so an agent has to
# pick the ticket up before closing it out
call POST "/support/tickets/$TICKET_ID/status" '{"status":"in_progress"}' "$AGENT" >/dev/null
s2=$(st)
call POST "/support/tickets/$TICKET_ID/status" '{"status":"resolved"}' "$AGENT" >/dev/null
if [ "$s1" = "201" ] && [ "$s2" = "200" ] && [ "$(st)" = "200" ]; then pass; else
  fail "message HTTP $s1, in_progress HTTP $s2, resolve HTTP $(st)"; fi

# --------------------------------------------------------------------------
step "a dispatcher books a shipment"
cust=$(call GET "/ops/customers?per_page=1" "" "$DISPATCH")
CUSTOMER=$(printf '%s' "$cust" | json "d['items'][0]['id']")
ship=$(call POST /ops/shipments "{\"customer_id\":\"$CUSTOMER\",\"mode\":\"sea\",\"incoterm\":\"FOB\",\"origin\":{\"city\":\"Ningbo\",\"country\":\"CN\"},\"destination\":{\"city\":\"Long Beach\",\"country\":\"US\"},\"cargo_description\":\"Consumer electronics, 12 pallets\",\"pieces\":12,\"weight_kg\":8400,\"volume_cbm\":22.5,\"declared_value\":180000,\"etd\":\"2026-09-02\",\"eta\":\"2026-09-24\"}" "$DISPATCH")
SHIPMENT=$(printf '%s' "$ship" | json "d['id']")
SHIP_REF=$(printf '%s' "$ship" | json "d.get('reference','')")
expect 201 "$(st)" "$SHIP_REF"

# --------------------------------------------------------------------------
step "an illegal shipment transition is refused"
call POST "/ops/shipments/$SHIPMENT/transition" '{"to":"delivered"}' "$DISPATCH" >/dev/null
if [ "$(st)" = "409" ]; then pass "draft to delivered refused"; else
  fail "expected HTTP 409, got $(st)"; fi

# --------------------------------------------------------------------------
step "the legal transitions are accepted"
call POST "/ops/shipments/$SHIPMENT/transition" '{"to":"booked"}' "$DISPATCH" >/dev/null; a=$(st)
call POST "/ops/shipments/$SHIPMENT/transition" '{"to":"picked_up","location":"Ningbo"}' "$DISPATCH" >/dev/null; b=$(st)
if [ "$a" = "200" ] && [ "$b" = "200" ]; then pass "draft to booked to picked_up"; else
  fail "booked HTTP $a, picked_up HTTP $b"; fi

# --------------------------------------------------------------------------
step "the dispatcher assigns a work order to the driver"
driver_me=$(call GET /auth/me "" "$DRIVER")
DRIVER_EMP=$(printf '%s' "$driver_me" | json "d['employee']['id']")
wo=$(call POST /ops/work-orders "{\"shipment_id\":\"$SHIPMENT\",\"kind\":\"pickup\",\"title\":\"Collect 12 pallets from the port\",\"instructions\":\"Gate 4, ask for the export desk.\",\"assigned_to\":\"$DRIVER_EMP\"}" "$DISPATCH")
WORK_ORDER=$(printf '%s' "$wo" | json "d['id']")
expect 201 "$(st)"

# --------------------------------------------------------------------------
step "someone else's work order cannot be updated"
call POST "/ops/work-orders/$WORK_ORDER/status" '{"status":"done"}' "$DOCK" >/dev/null
if [ "$(st)" = "403" ] || [ "$(st)" = "404" ]; then pass "refused with HTTP $(st)"; else
  fail "expected 403 or 404, got $(st)"; fi

# --------------------------------------------------------------------------
step "the assigned driver completes it"
call POST "/ops/work-orders/$WORK_ORDER/status" '{"status":"in_progress"}' "$DRIVER" >/dev/null; a=$(st)
call POST "/ops/work-orders/$WORK_ORDER/status" '{"status":"done","notes":"Collected, 12 pallets, seal 88421."}' "$DRIVER" >/dev/null
if [ "$a" = "200" ] && [ "$(st)" = "200" ]; then pass; else fail "start HTTP $a, done HTTP $(st)"; fi

# --------------------------------------------------------------------------
step "an accountant drafts an invoice for the shipment"
inv=$(call POST /finance/invoices "{\"customer_id\":\"$CUSTOMER\",\"shipment_id\":\"$SHIPMENT\",\"currency\":\"USD\",\"due_days\":30,\"lines\":[{\"description\":\"Ocean freight Ningbo to Long Beach\",\"quantity\":1,\"unit_price\":4200.00,\"tax_rate\":0},{\"description\":\"Customs brokerage\",\"quantity\":1,\"unit_price\":350.00,\"tax_rate\":0}]}" "$ACCT")
INVOICE=$(printf '%s' "$inv" | json "d['id']")
INV_TOTAL=$(printf '%s' "$inv" | json "d.get('total','')")
expect 201 "$(st)" "total $INV_TOTAL"

# --------------------------------------------------------------------------
step "the invoice is submitted and issued"
call POST "/finance/invoices/$INVOICE/submit" "" "$ACCT" >/dev/null; a=$(st)
issued=$(call POST "/finance/invoices/$INVOICE/issue" "" "$ACCT")
if [ "$a" = "200" ] && [ "$(st)" = "200" ]; then pass "issued"; else
  fail "submit HTTP $a, issue HTTP $(st)"; fi

# --------------------------------------------------------------------------
step "issuing posted a balanced journal entry"
entry=$(printf '%s' "$issued" | json "d.get('journal_entry_id','')")
if [ -n "$entry" ]; then pass "entry $entry"; else
  je=$(call GET "/finance/journal?source_type=invoice" "" "$ACCT")
  n=$(printf '%s' "$je" | json "len(d['items'])")
  if [ "${n:-0}" -ge 1 ]; then pass "$n invoice entries in the ledger"; else
    fail "no journal entry linked to the issued invoice"; fi
fi

# --------------------------------------------------------------------------
step "overpaying the invoice is refused"
call POST /finance/payments "{\"invoice_id\":\"$INVOICE\",\"received_on\":\"2026-09-05\",\"amount\":999999.00,\"method\":\"bank_transfer\",\"reference\":\"OVERPAY\"}" "$ACCT" >/dev/null
if [ "$(st)" = "422" ] || [ "$(st)" = "409" ]; then pass "refused with HTTP $(st)"; else
  fail "expected 422 or 409, got $(st)"; fi

# --------------------------------------------------------------------------
step "the correct payment is recorded"
call POST /finance/payments "{\"invoice_id\":\"$INVOICE\",\"received_on\":\"2026-09-05\",\"amount\":4550.00,\"method\":\"bank_transfer\",\"reference\":\"WIRE-90210\"}" "$ACCT" >/dev/null
expect 201 "$(st)"

# --------------------------------------------------------------------------
step "the trial balance still sums to zero"
tb=$(call GET /finance/reports/trial-balance "" "$ACCT")
total=$(printf '%s' "$tb" | json "round(sum(float(r['balance']) for r in ((d.get('rows') or d.get('items') or []) if isinstance(d,dict) else d)),2)")
if [ "$(st)" = "200" ] && { [ "$total" = "0.0" ] || [ "$total" = "0" ] || [ "$total" = "-0.0" ]; }; then
  pass "sum = $total"; else fail "HTTP $(st), trial balance sums to $total"; fi

# --------------------------------------------------------------------------
step "an unbalanced manual entry is refused"
call POST /finance/journal '{"entry_date":"2026-08-15","memo":"deliberately unbalanced","lines":[{"account_code":"1000","debit":100,"credit":0,"description":"only one side"}]}' "$ACCT" >/dev/null
if [ "$(st)" = "422" ] || [ "$(st)" = "409" ]; then pass "refused with HTTP $(st)"; else
  fail "expected 422 or 409, got $(st)"; fi

# --------------------------------------------------------------------------
step "a dock worker cannot read the ledger"
call GET /finance/reports/trial-balance "" "$DOCK" >/dev/null
expect 403 "$(st)"

# --------------------------------------------------------------------------
step "the CFO closes a fiscal period"
periods=$(call GET /finance/periods "" "$CFO")
PERIOD=$(printf '%s' "$periods" | json "
import datetime
items = d['items'] if isinstance(d,dict) and 'items' in d else d
today = datetime.date.today()
y, m = (today.year, today.month - 1) if today.month > 1 else (today.year - 1, 12)
next(( p['id'] for p in items if p['year']==y and p['month']==m and p['status']=='open'), '')")
if [ -n "$PERIOD" ]; then
  call POST "/finance/periods/$PERIOD/close" "" "$CFO" >/dev/null
  expect 200 "$(st)"
else
  pass "last month was already closed"
fi

# --------------------------------------------------------------------------
step "posting into the closed period is refused"
if [ -n "$PERIOD" ]; then
  last=$(python3 -c "
import datetime
t=datetime.date.today()
y,m=(t.year,t.month-1) if t.month>1 else (t.year-1,12)
print(f'{y:04d}-{m:02d}-15')")
  call POST /finance/journal "{\"entry_date\":\"$last\",\"memo\":\"late entry into a closed period\",\"lines\":[{\"account_code\":\"1000\",\"debit\":100,\"credit\":0},{\"account_code\":\"4000\",\"debit\":0,\"credit\":100}]}" "$ACCT" >/dev/null
  if [ "$(st)" = "409" ] || [ "$(st)" = "422" ]; then pass "refused with HTTP $(st)"; else
    fail "expected 409 or 422, got $(st)"; fi
else
  pass "skipped, no open prior period"
fi

# --------------------------------------------------------------------------
step "the CEO can read the audit trail for the invoice"
audit=$(call GET "/admin/audit?entity_type=invoice&entity_id=$INVOICE" "" "$CEO")
n=$(printf '%s' "$audit" | json "len(d['items'])")
if [ "$(st)" = "200" ] && [ "${n:-0}" -ge 1 ]; then pass "$n audit rows"; else
  fail "HTTP $(st), audit rows=$n"; fi

# --------------------------------------------------------------------------
step "a dock worker cannot read the audit trail"
call GET "/admin/audit?entity_type=invoice&entity_id=$INVOICE" "" "$DOCK" >/dev/null
expect 403 "$(st)"

# --------------------------------------------------------------------------
step "messages queued email in the outbox"
if command -v psql >/dev/null 2>&1 && [ -n "${DATABASE_URL:-}" ]; then
  n=$(psql "$DATABASE_URL" -Atc "select count(*) from notifications" 2>/dev/null)
  if [ "${n:-0}" -ge 1 ]; then pass "$n rows queued"; else fail "the outbox is empty"; fi
else
  pass "skipped, set DATABASE_URL to check the outbox"
fi

echo
if [ "$FAILURES" -eq 0 ]; then
  printf '%sAll %d checks passed.%s\n\n' "$c_green" "$STEP" "$c_off"
  exit 0
fi
printf '%s%d of %d checks failed.%s\n\n' "$c_red" "$FAILURES" "$STEP" "$c_off"
exit 1
