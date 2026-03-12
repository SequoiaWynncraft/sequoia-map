#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")
POSTGRES_SERVICE="${POSTGRES_SERVICE:-postgres}"
POSTGRES_DB="${POSTGRES_DB:-sequoia}"
POSTGRES_USER="${POSTGRES_USER:-sequoia}"
SERVER_SERVICE="${SERVER_SERVICE:-server}"

psql_query() {
  local sql="$1"
  "${COMPOSE[@]}" exec -T "$POSTGRES_SERVICE" \
    psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "$sql"
}

if ! "${COMPOSE[@]}" ps "$POSTGRES_SERVICE" | grep -q "running"; then
  echo "[verify] postgres service '$POSTGRES_SERVICE' is not running."
  exit 1
fi

if ! "${COMPOSE[@]}" ps "$SERVER_SERVICE" | grep -q "running"; then
  echo "[verify] server service '$SERVER_SERVICE' is not running."
  exit 1
fi

echo "[verify] Checking PostgreSQL major version"
server_version_num="$(psql_query "SHOW server_version_num;")"
server_major="$((server_version_num / 10000))"
if [[ "$server_major" -ne 18 ]]; then
  echo "[verify] Expected PostgreSQL major 18, got $server_major"
  exit 1
fi
server_version="$(psql_query "SHOW server_version;")"
echo "[verify] server_version=$server_version"

echo "[verify] Checking data_checksums and basic DB query"
checksums="$(psql_query "SHOW data_checksums;")"
echo "[verify] data_checksums=$checksums"
psql_query "SELECT 1;" >/dev/null

echo "[verify] API smoke: /api/health"
"${COMPOSE[@]}" exec -T "$SERVER_SERVICE" sh -ec 'curl -fsS http://127.0.0.1:3000/api/health >/dev/null'

echo "[verify] API smoke: /api/live/state"
"${COMPOSE[@]}" exec -T "$SERVER_SERVICE" sh -ec 'curl -fsS http://127.0.0.1:3000/api/live/state >/dev/null'

echo "[verify] API smoke: /api/events (SSE headers)"
"${COMPOSE[@]}" exec -T "$SERVER_SERVICE" sh -ec '
  headers="$(curl -sS --max-time 3 -D - -o /dev/null http://127.0.0.1:3000/api/events)"
  echo "$headers" | grep -qi "^content-type: text/event-stream"
'

echo "[verify] SPA deep-link smoke: /history returns HTML shell"
"${COMPOSE[@]}" exec -T "$SERVER_SERVICE" sh -ec '
  body="$(curl -fsS http://127.0.0.1:3000/history)"
  echo "$body" | grep -qi "<html"
'

echo "[verify] Backup service smoke"
"$ROOT_DIR/ops/backup/list_backups.sh" >/dev/null

echo "[verify] PostgreSQL 18 post-cutover checks passed"
