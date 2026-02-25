#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

BACKUP_PATH="${1:-latest}"

if [[ "$BACKUP_PATH" == "latest" ]]; then
  BACKUP_PATH="$(docker compose exec -T postgres-backup sh -ec 'ls -1t /backups/sequoia_*.sql.gz 2>/dev/null | head -n1')"
fi

if [[ -z "$BACKUP_PATH" ]]; then
  echo "No backup file found."
  exit 1
fi

echo "Restoring backup: $BACKUP_PATH"
echo "This restores into the current database and will drop/recreate dumped objects."

docker compose exec -T postgres-backup sh -ec 'gzip -dc "$1"' sh "$BACKUP_PATH" \
  | docker compose exec -T postgres psql -v ON_ERROR_STOP=1 -U sequoia -d sequoia

echo "Restore complete."
