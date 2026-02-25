#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

echo "Creating on-demand PostgreSQL backup..."
docker compose exec -T postgres-backup sh -ec '
  ts=$(date -u +%Y%m%dT%H%M%SZ)
  file="/backups/sequoia_${ts}.sql"
  pg_dump -h "$POSTGRES_HOST" -p "$POSTGRES_PORT" -U "$POSTGRES_USER" -d "$POSTGRES_DB" --clean --if-exists --no-owner --no-privileges > "$file"
  gzip -f "$file"
  echo "[backup] wrote ${file}.gz"
'
