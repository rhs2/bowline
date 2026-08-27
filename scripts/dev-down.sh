#!/usr/bin/env bash
# Stop the application services started by dev-up.sh. Leaves the Docker
# containers and the database alone; use `docker compose down` for those.
set -uo pipefail
cd "$(dirname "$0")/.."
LOGS="$PWD/.dev-logs"
for pid_file in "$LOGS"/*.pid; do
  [ -e "$pid_file" ] || continue
  name=$(basename "$pid_file" .pid)
  pid=$(cat "$pid_file")
  if kill -0 "$pid" 2>/dev/null; then
    pkill -TERM -P "$pid" 2>/dev/null
    kill -TERM "$pid" 2>/dev/null
    printf '  stopped %s (pid %s)\n' "$name" "$pid"
  else
    printf '  %s was not running\n' "$name"
  fi
  rm -f "$pid_file"
done
# the dev server and the maven wrapper spawn children that outlive their parent
pkill -f "bowline-api" 2>/dev/null
pkill -f "spring-boot:run" 2>/dev/null
pkill -f "analytics.main" 2>/dev/null
pkill -f "next dev" 2>/dev/null
echo "  done. Docker containers are still up; run 'docker compose down' to stop those too."
