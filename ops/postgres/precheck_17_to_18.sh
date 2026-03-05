#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")
POSTGRES_SERVICE="${POSTGRES_SERVICE:-postgres}"
POSTGRES_DB="${POSTGRES_DB:-sequoia}"
POSTGRES_USER="${POSTGRES_USER:-sequoia}"

psql_query() {
  local sql="$1"
  "${COMPOSE[@]}" exec -T "$POSTGRES_SERVICE" \
    psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "$sql"
}

if ! "${COMPOSE[@]}" ps "$POSTGRES_SERVICE" | grep -q "running"; then
  echo "[precheck] postgres service '$POSTGRES_SERVICE' is not running in compose file '$COMPOSE_FILE'."
  echo "[precheck] Start the stack first, then rerun this script."
  exit 1
fi

echo "[precheck] Running PostgreSQL 17 -> 18 preflight checks"

echo "\n== Server Version =="
server_version_num="$(psql_query "SHOW server_version_num;")"
server_major="$((server_version_num / 10000))"
server_version="$(psql_query "SHOW server_version;")"
echo "server_version=$server_version"
if [[ "$server_major" -ne 17 ]]; then
  echo "[precheck] Expected major version 17 before upgrade, got $server_major."
  exit 1
fi

echo "\n== Data Checksum Mode =="
checksums="$(psql_query "SHOW data_checksums;")"
echo "data_checksums=$checksums"

echo "\n== Installed Extensions =="
extensions="$(psql_query "SELECT extname || ':' || extversion FROM pg_extension ORDER BY extname;")"
if [[ -n "$extensions" ]]; then
  echo "$extensions"
else
  echo "(none)"
fi

non_core_extensions="$(psql_query "SELECT extname FROM pg_extension WHERE extname NOT IN ('plpgsql') ORDER BY extname;")"
if [[ -n "$non_core_extensions" ]]; then
  echo "[precheck] Non-core extensions detected:"
  echo "$non_core_extensions"
  echo "[precheck] Install matching PostgreSQL 18 extension binaries before cutover."
  echo "[precheck] Blocking upgrade until extension compatibility is confirmed."
  exit 1
fi

echo "\n== MD5 Authentication Rules =="
md5_rules="$(psql_query "SELECT coalesce(line_number::text,'?') || ':' || type || ':' || database || ':' || user_name || ':' || auth_method FROM pg_hba_file_rules WHERE auth_method = 'md5' ORDER BY line_number;")"
if [[ -n "$md5_rules" ]]; then
  echo "$md5_rules"
  echo "[precheck] WARNING: md5 auth is deprecated in PostgreSQL 18. Plan migration to scram-sha-256."
else
  echo "No md5 rules found."
fi

echo "\n== Unlogged Partitioned Tables =="
unlogged_partitioned="$(psql_query "SELECT format('%I.%I', n.nspname, c.relname) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace WHERE c.relkind = 'p' AND c.relpersistence = 'u' ORDER BY 1;")"
if [[ -n "$unlogged_partitioned" ]]; then
  echo "$unlogged_partitioned"
  echo "[precheck] PostgreSQL 18 disallows unlogged partitioned tables. Resolve these before upgrade."
  exit 1
else
  echo "No unlogged partitioned tables found."
fi

echo "\n== Collation / pg_trgm Reindex Advisory =="
default_collation_provider="$(psql_query "SELECT setting FROM pg_settings WHERE name = 'default_collation_provider';")"
if [[ -z "$default_collation_provider" ]]; then
  default_collation_provider="(not reported by server)"
fi
pg_trgm_present="$(psql_query "SELECT EXISTS (SELECT 1 FROM pg_extension WHERE extname = 'pg_trgm');")"
echo "default_collation_provider=$default_collation_provider"
echo "pg_trgm_installed=$pg_trgm_present"
if [[ "$pg_trgm_present" == "t" ]] && [[ "$default_collation_provider" != "libc" ]]; then
  echo "[precheck] WARNING: non-libc collation provider + pg_trgm detected."
  echo "[precheck] Reindex pg_trgm/full-text indexes after upgrade (per PostgreSQL 18 migration notes)."
fi

echo "\n[precheck] All blocking checks passed."
