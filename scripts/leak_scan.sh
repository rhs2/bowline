#!/usr/bin/env bash
# Refuse to publish anything that should not leave this machine.
#
#   ./scripts/leak_scan.sh              scan the files git would track
#   ./scripts/leak_scan.sh --staged     scan only what is staged (used by the hook)
#   ./scripts/leak_scan.sh --all        scan the whole working tree, ignored files too
#
# Exit codes: 0 clean, 1 findings.
#
# The scan deliberately runs over the files GIT WOULD PUBLISH, not the working
# tree. A secret sitting in an ignored file is not a leak; the same secret in a
# tracked file is. `--all` exists for the paranoid pass before a first push.
set -uo pipefail
cd "$(dirname "$0")/.."

MODE="${1:---tracked}"
FINDINGS=0
red=$'\033[31m'; yellow=$'\033[33m'; green=$'\033[32m'; dim=$'\033[2m'; off=$'\033[0m'
[ -t 1 ] || { red=""; yellow=""; green=""; dim=""; off=""; }

case "$MODE" in
  --staged)  FILES=$(git diff --cached --name-only --diff-filter=ACM 2>/dev/null) ;;
  --all)     FILES=$(find . -type f -not -path "./.git/*" -not -path "*/node_modules/*" \
                       -not -path "*/target/*" -not -path "*/.venv/*" -not -path "*/.next/*" \
                       -not -path "*/__pycache__/*" -not -path "*/.dev-logs/*" | sed 's|^\./||') ;;
  *)         FILES=$(git ls-files 2>/dev/null || true)
             [ -z "$FILES" ] && FILES=$(git add -A --dry-run 2>/dev/null | sed "s/^add '//;s/'$//") ;;
esac

if [ -z "${FILES:-}" ]; then
  echo "Nothing to scan. Is this a git repository?"
  exit 0
fi
COUNT=$(printf '%s\n' "$FILES" | grep -c . || true)

# Skip binary and generated files: a lockfile hash is not a secret, and grepping
# a 300 KB lockfile for entropy produces nothing but noise.
scannable() {
  case "$1" in
    *.lock|*lock.json|go.sum|*.png|*.jpg|*.jpeg|*.gif|*.ico|*.pdf|*.woff*|*.ttf|*.zip|*.jar|*.joblib) return 1 ;;
  esac
  [ -f "$1" ] && ! grep -qI . "$1" 2>/dev/null && return 1
  return 0
}

SCAN_LIST=$(mktemp)
printf '%s\n' "$FILES" | while read -r f; do scannable "$f" && echo "$f"; done > "$SCAN_LIST"

# hit NAME SEVERITY PATTERN [inverse-filter] [ci]
#   Reports every match of PATTERN. The optional filter removes known-safe lines
#   and is matched against the content only. Pass "ci" to match case insensitively,
#   which is what catches an uppercase constant such as `PASSWORD: &str = "..."`.
hit() {
  local name="$1" sev="$2" pattern="$3" filter="${4:-}" ci="${5:-}"
  local out flags="-InE"
  [ "$ci" = "ci" ] && flags="-IniE"
  out=$(grep $flags "$pattern" $(cat "$SCAN_LIST" | tr '\n' ' ') 2>/dev/null || true)
  # Apply the filter to the matched CONTENT, never to the path. A file called
  # something_test.rs must not get a free pass just because "test" is in its name.
  if [ -n "$filter" ] && [ -n "$out" ]; then
    out=$(printf '%s\n' "$out" | python3 -c '
import re, sys
pat = re.compile(sys.argv[1], re.I)
for line in sys.stdin.read().splitlines():
    if not line:
        continue
    content = re.sub(r"^[^:]*:[0-9]+:", "", line)
    if not pat.search(content):
        print(line)
' "$filter" || true)
  fi
  out=$(printf '%s\n' "$out" | grep -v '^$' || true)
  if [ -n "$out" ]; then
    local colour="$red"; [ "$sev" = "warn" ] && colour="$yellow"
    printf '%s%s%s  %s\n' "$colour" "$([ "$sev" = warn ] && echo WARN || echo LEAK)" "$off" "$name"
    printf '%s\n' "$out" | head -6 | sed 's/^/      /'
    local n; n=$(printf '%s\n' "$out" | grep -c . || true)
    [ "$n" -gt 6 ] && printf '      %s... and %s more%s\n' "$dim" "$((n - 6))" "$off"
    [ "$sev" = "warn" ] || FINDINGS=$((FINDINGS + 1))
  fi
}

echo
printf 'Scanning %s files (%s)\n\n' "$COUNT" "${MODE#--}"

# --- 1. credentials that are unambiguous ------------------------------------
hit "private key material" leak \
  '\-\-\-\-\-BEGIN [A-Z ]*(PRIVATE KEY|RSA PRIVATE KEY|OPENSSH PRIVATE KEY)'
hit "AWS access key id" leak \
  '(AKIA|ASIA|ABIA|ACCA)[0-9A-Z]{16}' \
  'AKIA\.\.\.|"AKIA"|AKIAIOSFODNN7EXAMPLE'
hit "AWS secret access key" leak \
  'aws_secret_access_key[[:space:]]*=[[:space:]]*[A-Za-z0-9/+=]{40}'
hit "GitHub token" leak  'gh[pousr]_[A-Za-z0-9]{36,}'
hit "Slack token" leak   'xox[baprs]-[A-Za-z0-9-]{10,}'
hit "LLM provider API key" leak 'sk-[A-Za-z0-9_-]{32,}|sk-ant-[A-Za-z0-9_-]{20,}'
hit "Google API key" leak 'AIza[0-9A-Za-z_-]{35}'
hit "Stripe key" leak     '(sk|rk)_(live|test)_[A-Za-z0-9]{20,}'
hit "JSON Web Token" leak 'eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'
hit "npm or PyPI token" leak 'npm_[A-Za-z0-9]{36}|pypi-AgEIcHlwaS5vcmc[A-Za-z0-9_-]{20,}'

# --- 2. real infrastructure -------------------------------------------------
# 000000000000 and 111111111111 are the documented placeholders.
hit "AWS account id" leak \
  '(^|[^0-9])[0-9]{12}([^0-9]|$)' \
  '000000000000|111111111111|123456789012|[0-9]{13}|sha256|integrity|[0-9a-f]{12}'
hit "AWS ARN with a real account" leak \
  'arn:aws[a-z-]*:[a-z0-9-]+:[a-z0-9-]*:[0-9]{12}:' \
  '000000000000|111111111111|\$\{|var\.|local\.|data\.|aws_'
hit "RDS or ElastiCache endpoint" leak \
  '[a-z0-9-]+\.[a-z0-9]+\.[a-z]{2}-[a-z]+-[0-9]\.(rds|cache)\.amazonaws\.com'

# --- 3. personal identity ---------------------------------------------------
# The terms to look for are NOT listed here on purpose: writing a real name or a
# company into this file would publish the very thing the check exists to catch.
# They live in scripts/private_patterns.txt, which .gitignore excludes. One
# extended-regex alternation per line, blank lines and # comments ignored.
# Without that file this check is skipped and says so.
PRIVATE_PATTERNS_FILE="scripts/private_patterns.txt"
if [ -f "$PRIVATE_PATTERNS_FILE" ]; then
  PRIVATE=$(grep -vE '^[[:space:]]*(#|$)' "$PRIVATE_PATTERNS_FILE" | paste -sd'|' -)
  if [ -n "$PRIVATE" ]; then
    # the pattern file is itself excluded, or it would match every one of its own lines
    hit "personal or corporate identity" leak "$PRIVATE" "^$PRIVATE_PATTERNS_FILE:|^scripts/leak_scan\.sh:"
  fi
else
  printf '%sSKIP%s  personal identity check: %s not found\n' "$yellow" "$off" "$PRIVATE_PATTERNS_FILE"
  printf '      Create it (one term per line) so a real name or company cannot slip in.\n'
fi

# Reserved TLDs (RFC 2606 and RFC 6761) can never resolve, so fixtures using
# .example, .test, .invalid and .localhost are safe by construction.
hit "email address outside the reserved test domains" leak \
  '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
  '@[A-Za-z0-9.-]+\.(example|test|invalid|localhost)([^A-Za-z]|$)|@types|@typescript|@next|@tailwind|@eslint|@vitejs|@testing-library|@radix|@babel|@rollup|@esbuild|@img|@jridgewell|@nodelib|@pkgjs|@isaacs|@ampproject|@alloc|@emnapi|@napi|@nolyfill|@sinclair|@standard-schema|@swc|@tybys|@unrs|@vitest|noreply|no-reply|@media|@keyframes|@apply|@layer|@tailwindcss|@import|@font-face|@charset|@supports|@container|@page|@property|@scope|@starting'

# --- 4. hardcoded credentials in code ---------------------------------------
# Every real secret is read from the environment. A literal here is a mistake.
# The value must look like a credential: no spaces. That is what separates a
# real secret from a user-facing string such as "Enter your current password".
hit "hardcoded password or secret literal" leak \
  '(password|passwd|secret|api_?key|auth_?token)[A-Za-z_]*[[:space:]]*[:=][^"]{0,24}"[^"$<{ ][^" ]{7,}"' \
  '"[A-Z0-9_]{4,}"|secretsmanager|bowline_(app|ro|notify)_dev|dev-only-change-me|dev-internal-token|minioadmin|Bowline!2026|postgres:postgres|ci-only|ci-internal|password_hash|\.example|placeholder|your-|change-?me|xxx|redacted|random_password|var\.|local\.|process\.env|getenv|std::env|System\.getenv|test|spec|mock|fake|stub|dummy|sample' \
  ci

# --- 5. things that simply should not be tracked ----------------------------
BADFILES=$(printf '%s\n' "$FILES" | grep -E '(^|/)(\.env$|\.env\.[^e]|.*\.tfstate|.*\.tfvars$|id_rsa|id_ed25519|.*\.pem$|.*\.p12$|.*\.jks$|credentials\.json|\.netrc|\.npmrc|kubeconfig)' || true)
if [ -n "$BADFILES" ]; then
  printf '%sLEAK%s  file that must never be tracked\n' "$red" "$off"
  printf '%s\n' "$BADFILES" | sed 's/^/      /'
  FINDINGS=$((FINDINGS + 1))
fi

INTERNAL=$(printf '%s\n' "$FILES" | grep -E '^internal/' || true)
if [ -n "$INTERNAL" ]; then
  printf '%sLEAK%s  internal working notes are tracked (see internal/README.md)\n' "$red" "$off"
  printf '%s\n' "$INTERNAL" | sed 's/^/      /'
  FINDINGS=$((FINDINGS + 1))
fi

BIG=$(printf '%s\n' "$FILES" | while read -r f; do
        [ -f "$f" ] || continue
        s=$(stat -f%z "$f" 2>/dev/null || stat -c%s "$f" 2>/dev/null || echo 0)
        [ "$s" -gt 1048576 ] && printf '%s (%s KB)\n' "$f" "$((s / 1024))"
      done || true)
if [ -n "$BIG" ]; then
  printf '%sWARN%s  file over 1 MB, check it belongs in git\n' "$yellow" "$off"
  printf '%s\n' "$BIG" | sed 's/^/      /'
fi

rm -f "$SCAN_LIST"
echo
if [ "$FINDINGS" -eq 0 ]; then
  printf '%sClean.%s Nothing found that should not be published.\n\n' "$green" "$off"
  exit 0
fi
printf '%s%s finding(s).%s Fix them, or add the safe ones to the filter in this script\n' "$red" "$FINDINGS" "$off"
printf 'with a comment saying why they are safe. Do not silence a finding you have not read.\n\n'
exit 1
