#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

COMPOSE_FILE="${COMPOSE_FILE:-docker-compose.yml}"
COMPOSE=(docker compose -f "$COMPOSE_FILE")
POSTGRES_SERVICE="${POSTGRES_SERVICE:-postgres}"
POSTGRES_DB="${POSTGRES_DB:-sequoia}"
POSTGRES_USER="${POSTGRES_USER:-sequoia}"
UPGRADE_IMAGE="${UPGRADE_IMAGE:-tianon/postgres-upgrade:17-to-18}"
SNAPSHOT_DIR="${SNAPSHOT_DIR:-$ROOT_DIR/ops/postgres/snapshots}"

WRITER_CANDIDATES=(server ingest caddy edge)

service_exists() {
  local svc="$1"
  "${COMPOSE[@]}" config --services | grep -qx "$svc"
}

discover_postgres_container_id() {
  "${COMPOSE[@]}" ps -q "$POSTGRES_SERVICE"
}

discover_postgres_volume_name() {
  local container_id="$1"
  local volume_name
  volume_name="$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/var/lib/postgresql"}}{{.Name}}{{end}}{{end}}' "$container_id")"
  if [[ -z "$volume_name" ]]; then
    volume_name="$(docker inspect -f '{{range .Mounts}}{{if eq .Destination "/var/lib/postgresql/data"}}{{.Name}}{{end}}{{end}}' "$container_id")"
  fi
  echo "$volume_name"
}

psql_query() {
  local sql="$1"
  "${COMPOSE[@]}" exec -T "$POSTGRES_SERVICE" \
    psql -v ON_ERROR_STOP=1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -Atc "$sql"
}

run_upgrade() {
  local mode_args=("$@")
  docker run --rm \
    --env "POSTGRES_INITDB_ARGS=${POSTGRES_INITDB_ARGS}" \
    --mount "type=volume,src=${POSTGRES_VOLUME_NAME},dst=/var/lib/postgresql" \
    --env "PGDATAOLD=/var/lib/postgresql/17/docker" \
    --env "PGDATANEW=/var/lib/postgresql/18/docker" \
    "$UPGRADE_IMAGE" "${mode_args[@]}"
}

if ! "${COMPOSE[@]}" ps "$POSTGRES_SERVICE" | grep -q "running"; then
  echo "[upgrade] postgres service '$POSTGRES_SERVICE' is not running in compose file '$COMPOSE_FILE'."
  echo "[upgrade] Start the stack on PostgreSQL 17 before running this script."
  exit 1
fi

if [[ -x "$ROOT_DIR/ops/postgres/precheck_17_to_18.sh" ]]; then
  echo "[upgrade] Running precheck gate"
  "$ROOT_DIR/ops/postgres/precheck_17_to_18.sh"
fi

echo "[upgrade] Triggering logical backup"
"$ROOT_DIR/ops/backup/backup_now.sh"

server_version_num="$(psql_query "SHOW server_version_num;")"
server_major="$((server_version_num / 10000))"
if [[ "$server_major" -ne 17 ]]; then
  echo "[upgrade] Expected PostgreSQL major 17 before cutover, found $server_major"
  exit 1
fi

checksum_mode="$(psql_query "SHOW data_checksums;")"
if [[ "$checksum_mode" == "on" ]]; then
  POSTGRES_INITDB_ARGS="--data-checksums"
else
  POSTGRES_INITDB_ARGS="--no-data-checksums"
fi

echo "[upgrade] data_checksums=$checksum_mode -> POSTGRES_INITDB_ARGS='$POSTGRES_INITDB_ARGS'"

POSTGRES_CONTAINER_ID="$(discover_postgres_container_id)"
if [[ -z "$POSTGRES_CONTAINER_ID" ]]; then
  echo "[upgrade] Failed to resolve running postgres container id"
  exit 1
fi

POSTGRES_VOLUME_NAME="$(discover_postgres_volume_name "$POSTGRES_CONTAINER_ID")"
if [[ -z "$POSTGRES_VOLUME_NAME" ]]; then
  echo "[upgrade] Failed to resolve postgres data volume name from container mounts"
  exit 1
fi

echo "[upgrade] postgres container=$POSTGRES_CONTAINER_ID"
echo "[upgrade] postgres volume=$POSTGRES_VOLUME_NAME"

echo "[upgrade] Stopping writer services"
for svc in "${WRITER_CANDIDATES[@]}"; do
  if service_exists "$svc"; then
    "${COMPOSE[@]}" stop "$svc"
  fi
done

echo "[upgrade] Stopping postgres"
"${COMPOSE[@]}" stop "$POSTGRES_SERVICE"

mkdir -p "$SNAPSHOT_DIR"
timestamp="$(date -u +%Y%m%dT%H%M%SZ)"
snapshot_file="$SNAPSHOT_DIR/pgdata_${timestamp}.tar.gz"

echo "[upgrade] Creating volume snapshot: $snapshot_file"
docker run --rm \
  --mount "type=volume,src=${POSTGRES_VOLUME_NAME},dst=/source,readonly" \
  alpine:3.22 sh -ec 'cd /source && tar -czf - .' > "$snapshot_file"

echo "[upgrade] Normalizing on-disk layout for pg_upgrade"
docker run --rm \
  --mount "type=volume,src=${POSTGRES_VOLUME_NAME},dst=/var/lib/postgresql" \
  alpine:3.22 sh -ec '
    set -eu
    mkdir -p /var/lib/postgresql/17/docker /var/lib/postgresql/18/docker

    if [ -f /var/lib/postgresql/PG_VERSION ] && [ ! -f /var/lib/postgresql/17/docker/PG_VERSION ]; then
      echo "[upgrade] Migrating legacy root layout into /17/docker"
      for entry in /var/lib/postgresql/*; do
        [ -e "$entry" ] || continue
        base="$(basename "$entry")"
        case "$base" in
          17|18)
            continue
            ;;
        esac
        mv "$entry" /var/lib/postgresql/17/docker/
      done
    fi

    if [ -f /var/lib/postgresql/data/PG_VERSION ] && [ ! -f /var/lib/postgresql/17/docker/PG_VERSION ]; then
      echo "[upgrade] Migrating /data layout into /17/docker"
      for entry in /var/lib/postgresql/data/*; do
        [ -e "$entry" ] || continue
        mv "$entry" /var/lib/postgresql/17/docker/
      done
      rmdir /var/lib/postgresql/data || true
    fi

    if [ ! -f /var/lib/postgresql/17/docker/PG_VERSION ]; then
      echo "[upgrade] Missing /var/lib/postgresql/17/docker/PG_VERSION after normalization"
      exit 1
    fi
  '

echo "[upgrade] Running pg_upgrade --check --link"
run_upgrade --check --link

echo "[upgrade] Running pg_upgrade --link"
if run_upgrade --link; then
  echo "[upgrade] pg_upgrade --link succeeded"
else
  echo "[upgrade] pg_upgrade --link failed; cleaning target dir and retrying with --copy"
  docker run --rm \
    --mount "type=volume,src=${POSTGRES_VOLUME_NAME},dst=/var/lib/postgresql" \
    alpine:3.22 sh -ec 'rm -rf /var/lib/postgresql/18/docker && mkdir -p /var/lib/postgresql/18/docker'
  run_upgrade --copy
fi

echo "[upgrade] Starting postgres on 18"
"${COMPOSE[@]}" up -d "$POSTGRES_SERVICE"

echo "[upgrade] Waiting for postgres readiness"
ready=0
for _ in $(seq 1 60); do
  if "${COMPOSE[@]}" exec -T "$POSTGRES_SERVICE" pg_isready -U "$POSTGRES_USER" -d "$POSTGRES_DB" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 2
done
if [[ "$ready" -ne 1 ]]; then
  echo "[upgrade] postgres did not become ready in time"
  exit 1
fi

echo "[upgrade] Starting application services"
"${COMPOSE[@]}" up -d

echo "[upgrade] Running post-cutover verification"
"$ROOT_DIR/ops/postgres/verify_18_postcutover.sh"

echo "[upgrade] Completed successfully"
echo "[upgrade] Volume snapshot retained at: $snapshot_file"
