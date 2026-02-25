#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$ROOT_DIR"

docker compose exec -T postgres-backup sh -ec '
  ls -lh /backups/sequoia_*.sql.gz 2>/dev/null || {
    echo "No backups found in /backups";
    exit 0;
  }
'
