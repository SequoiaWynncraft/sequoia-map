#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="${ROOT_DIR}/.env.dev.local"
COMPOSE_FILE="${ROOT_DIR}/docker-compose.dev.yml"
PROJECT_NAME="sequoia-map-mod-ingest"
PGDATA_VOLUME="${PROJECT_NAME}_pgdata-dev"
POSTGRES_MAJOR="18"
POSTGRES_IMAGE="postgres:${POSTGRES_MAJOR}-alpine"
POSTGRES_VOLUME_ROOT="/var/lib/postgresql"
POSTGRES_PGDATA_DEFAULT="${POSTGRES_VOLUME_ROOT}/${POSTGRES_MAJOR}/docker"
POSTGRES_PGDATA_LEGACY_ROOT="${POSTGRES_VOLUME_ROOT}"
POSTGRES_PGDATA_LEGACY_SUBDIR="${POSTGRES_VOLUME_ROOT}/pgdata"
DOCKER_BIN=()
COMPOSE_BIN=()
POSTGRES_VOLUME_STATE=""

resolve_docker_bin() {
  if [[ ${#DOCKER_BIN[@]} -gt 0 ]]; then
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    DOCKER_BIN=(docker)
    return
  fi

  echo "Docker CLI is required but was not found in PATH." >&2
  exit 1
}

resolve_compose_bin() {
  if [[ ${#COMPOSE_BIN[@]} -gt 0 ]]; then
    return
  fi

  resolve_docker_bin

  if "${DOCKER_BIN[@]}" compose version >/dev/null 2>&1; then
    COMPOSE_BIN=("${DOCKER_BIN[@]}" compose)
    return
  fi

  if command -v docker-compose >/dev/null 2>&1; then
    COMPOSE_BIN=(docker-compose)
    return
  fi

  cat >&2 <<'EOF'
Docker Compose is required but was not found.
Install either the Docker Compose v2 plugin (`docker compose`) or the standalone `docker-compose` binary.
EOF
  exit 1
}

docker_daemon_available() {
  resolve_docker_bin
  "${DOCKER_BIN[@]}" info >/dev/null 2>&1
}

require_docker_daemon() {
  if docker_daemon_available; then
    return
  fi

  cat >&2 <<'EOF'
Docker is installed, but the Docker daemon is not reachable.
Start Docker Desktop or the Docker service, then rerun ./dev.sh.
EOF
  exit 1
}

compose_command_requires_daemon() {
  local subcommand="${1:-up}"
  case "${subcommand}" in
    config|convert|help|version)
      return 1
      ;;
    *)
      return 0
      ;;
  esac
}

random_hex() {
  local bytes="${1}"
  if command -v openssl >/dev/null 2>&1; then
    openssl rand -hex "${bytes}"
    return
  fi

  python3 - "${bytes}" <<'PY'
import secrets
import sys

print(secrets.token_hex(int(sys.argv[1])))
PY
}

port_in_use() {
  local port="${1}"
  if command -v ss >/dev/null 2>&1; then
    ss -ltn "( sport = :${port} )" | tail -n +2 | grep -q .
    return
  fi

  if command -v lsof >/dev/null 2>&1; then
    lsof -iTCP:"${port}" -sTCP:LISTEN -t >/dev/null 2>&1
    return
  fi

  return 1
}

select_postgres_port() {
  local candidate
  for candidate in $(seq 55432 55464); do
    if ! port_in_use "${candidate}"; then
      printf '%s\n' "${candidate}"
      return
    fi
  done

  echo "Unable to find a free local Postgres port in the 55432-55464 range." >&2
  exit 1
}

find_service_container() {
  local service="${1}"
  "${DOCKER_BIN[@]}" ps -a \
    --filter "label=com.docker.compose.project=${PROJECT_NAME}" \
    --filter "label=com.docker.compose.service=${service}" \
    --format '{{.Names}}' \
    | head -n1
}

read_container_env() {
  local container="${1}"
  local key="${2}"

  "${DOCKER_BIN[@]}" inspect --format '{{range .Config.Env}}{{println .}}{{end}}' "${container}" \
    | sed -n "s/^${key}=//p" \
    | tail -n1
}

read_postgres_port_from_container() {
  local container="${1}"

  "${DOCKER_BIN[@]}" port "${container}" 5432/tcp \
    | awk -F: 'NR == 1 { print $NF }'
}

detect_postgres_volume_state() {
  if [[ -n "${POSTGRES_VOLUME_STATE}" ]]; then
    printf '%s\n' "${POSTGRES_VOLUME_STATE}"
    return
  fi

  resolve_docker_bin

  if ! "${DOCKER_BIN[@]}" volume inspect "${PGDATA_VOLUME}" >/dev/null 2>&1; then
    POSTGRES_VOLUME_STATE="missing"
    printf '%s\n' "${POSTGRES_VOLUME_STATE}"
    return
  fi

  POSTGRES_VOLUME_STATE="$("${DOCKER_BIN[@]}" run --rm --entrypoint sh \
    -e "POSTGRES_MAJOR=${POSTGRES_MAJOR}" \
    -v "${PGDATA_VOLUME}:/pgdata" \
    "${POSTGRES_IMAGE}" \
    -ceu '
if [ -f "/pgdata/${POSTGRES_MAJOR}/docker/PG_VERSION" ]; then
  echo initialized-versioned-subdir
elif [ -f /pgdata/PG_VERSION ]; then
  echo initialized-root
elif [ -f /pgdata/pgdata/PG_VERSION ]; then
  echo initialized-subdir
elif find /pgdata -mindepth 3 -maxdepth 3 -path '/pgdata/*/docker/PG_VERSION' | grep -q .; then
  echo initialized-other-versioned-subdir
elif [ -n "$(ls -A /pgdata 2>/dev/null)" ]; then
  echo nonempty-uninitialized
else
  echo empty
fi
')"

  printf '%s\n' "${POSTGRES_VOLUME_STATE}"
}

read_postgres_volume_pg_version() {
  local relative_path="${1}"

  resolve_docker_bin

  "${DOCKER_BIN[@]}" run --rm --entrypoint sh \
    -v "${PGDATA_VOLUME}:/pgdata" \
    "${POSTGRES_IMAGE}" \
    -ceu '
path="${1}"
if [ -n "${path}" ]; then
  cat "/pgdata/${path}/PG_VERSION"
else
  cat /pgdata/PG_VERSION
fi
' sh "${relative_path}"
}

configure_postgres_pgdata() {
  local volume_state
  local pg_version
  volume_state="$(detect_postgres_volume_state)"

  case "${volume_state}" in
    initialized-versioned-subdir)
      export POSTGRES_PGDATA="${POSTGRES_PGDATA_DEFAULT}"
      ;;
    initialized-root)
      pg_version="$(read_postgres_volume_pg_version "")"
      if [[ "${pg_version}" == "${POSTGRES_MAJOR}" ]]; then
        export POSTGRES_PGDATA="${POSTGRES_PGDATA_LEGACY_ROOT}"
      else
        export POSTGRES_PGDATA="${POSTGRES_PGDATA_DEFAULT}"
      fi
      ;;
    initialized-subdir)
      pg_version="$(read_postgres_volume_pg_version "pgdata")"
      if [[ "${pg_version}" == "${POSTGRES_MAJOR}" ]]; then
        export POSTGRES_PGDATA="${POSTGRES_PGDATA_LEGACY_SUBDIR}"
      else
        export POSTGRES_PGDATA="${POSTGRES_PGDATA_DEFAULT}"
      fi
      ;;
    missing|empty|initialized-other-versioned-subdir|nonempty-uninitialized)
      export POSTGRES_PGDATA="${POSTGRES_PGDATA_DEFAULT}"
      ;;
    *)
      echo "Unable to determine the state of Docker volume ${PGDATA_VOLUME}." >&2
      exit 1
      ;;
  esac

  case "${volume_state}" in
    initialized-root|initialized-subdir)
      if [[ "${POSTGRES_PGDATA}" == "${POSTGRES_PGDATA_DEFAULT}" ]]; then
        echo "Detected an older PostgreSQL ${pg_version} data directory in ${PGDATA_VOLUME}; using ${POSTGRES_PGDATA} so the dev stack boots with PostgreSQL ${POSTGRES_MAJOR}."
      fi
      ;;
    initialized-versioned-subdir)
      echo "Detected a PostgreSQL ${POSTGRES_MAJOR} data directory in ${PGDATA_VOLUME}; using ${POSTGRES_PGDATA}."
      ;;
    initialized-other-versioned-subdir)
      echo "Detected a versioned Postgres data directory for a different major in ${PGDATA_VOLUME}; using ${POSTGRES_PGDATA} for PostgreSQL ${POSTGRES_MAJOR}."
      ;;
    nonempty-uninitialized)
      echo "Detected a non-empty but uninitialized Postgres dev volume; using ${POSTGRES_PGDATA} so the stack can initialize cleanly."
      ;;
  esac
}

write_env_file() {
  local postgres_password="${1}"
  local internal_ingest_token="${2}"
  local postgres_port="${3}"
  local note="${4}"

  umask 077
  cat >"${ENV_FILE}" <<EOF
# ${note}
POSTGRES_PASSWORD=${postgres_password}
INTERNAL_INGEST_TOKEN=${internal_ingest_token}
POSTGRES_PORT=${postgres_port}
EOF
}

recover_env_file_from_existing_stack() {
  local postgres_container
  local server_container
  local ingest_container
  local postgres_password
  local internal_ingest_token
  local postgres_port

  postgres_container="$(find_service_container postgres)"
  server_container="$(find_service_container server)"
  ingest_container="$(find_service_container ingest)"

  if [[ -z "${postgres_container}" ]]; then
    return 1
  fi

  postgres_password="$(read_container_env "${postgres_container}" POSTGRES_PASSWORD)"
  postgres_port="$(read_postgres_port_from_container "${postgres_container}")"

  if [[ -n "${server_container}" ]]; then
    internal_ingest_token="$(read_container_env "${server_container}" INTERNAL_INGEST_TOKEN)"
  fi
  if [[ -z "${internal_ingest_token:-}" && -n "${ingest_container}" ]]; then
    internal_ingest_token="$(read_container_env "${ingest_container}" SEQUOIA_INTERNAL_INGEST_TOKEN)"
  fi

  if [[ -z "${postgres_password}" || -z "${internal_ingest_token:-}" || -z "${postgres_port}" ]]; then
    return 1
  fi

  write_env_file \
    "${postgres_password}" \
    "${internal_ingest_token}" \
    "${postgres_port}" \
    "Generated by ./dev.sh from the existing ${PROJECT_NAME} Docker stack. Safe to edit."
  echo "Created ${ENV_FILE} from the existing ${PROJECT_NAME} Docker stack."
  return 0
}

ensure_env_file() {
  local pg_version
  local volume_state

  if [[ -f "${ENV_FILE}" ]]; then
    return
  fi

  if docker_daemon_available; then
    if recover_env_file_from_existing_stack; then
      return
    fi

    volume_state="$(detect_postgres_volume_state)"

    case "${volume_state}" in
      initialized-versioned-subdir)
        cat >&2 <<EOF
Found existing Docker volume ${PGDATA_VOLUME} but could not recover the dev credentials for it.
Either remove that volume if you do not need the local database anymore, or create ${ENV_FILE}
manually with matching POSTGRES_PASSWORD, INTERNAL_INGEST_TOKEN, and POSTGRES_PORT values.
EOF
        exit 1
        ;;
      initialized-root|initialized-subdir)
        if [[ "${volume_state}" == "initialized-root" ]]; then
          pg_version="$(read_postgres_volume_pg_version "")"
        else
          pg_version="$(read_postgres_volume_pg_version "pgdata")"
        fi

        if [[ "${pg_version}" == "${POSTGRES_MAJOR}" ]]; then
          cat >&2 <<EOF
Found existing Docker volume ${PGDATA_VOLUME} but could not recover the dev credentials for it.
Either remove that volume if you do not need the local database anymore, or create ${ENV_FILE}
manually with matching POSTGRES_PASSWORD, INTERNAL_INGEST_TOKEN, and POSTGRES_PORT values.
EOF
          exit 1
        fi

        echo "Found a PostgreSQL ${pg_version} data directory in ${PGDATA_VOLUME}; generating fresh PostgreSQL ${POSTGRES_MAJOR} dev credentials instead of reusing it."
        ;;
      initialized-other-versioned-subdir|nonempty-uninitialized)
        echo "Found Docker volume ${PGDATA_VOLUME} without an initialized Postgres cluster at the active PGDATA path; generating fresh dev credentials."
        ;;
    esac
  fi

  local postgres_port
  postgres_port="$(select_postgres_port)"

  write_env_file \
    "$(random_hex 24)" \
    "$(random_hex 24)" \
    "${postgres_port}" \
    "Generated by ./dev.sh. Safe to edit for local development."

  echo "Created ${ENV_FILE} with stable dev credentials and POSTGRES_PORT=${postgres_port}."
}

main() {
  local compose_args=("$@")

  if [[ ${#compose_args[@]} -eq 0 ]]; then
    compose_args=(up --build)
  fi

  cd "${ROOT_DIR}"
  resolve_compose_bin

  if compose_command_requires_daemon "${compose_args[0]}"; then
    require_docker_daemon
  fi

  ensure_env_file

  if compose_command_requires_daemon "${compose_args[0]}"; then
    configure_postgres_pgdata
  else
    export POSTGRES_PGDATA="${POSTGRES_PGDATA_DEFAULT}"
  fi

  exec "${COMPOSE_BIN[@]}" \
    --project-name "${PROJECT_NAME}" \
    --project-directory "${ROOT_DIR}" \
    --env-file "${ENV_FILE}" \
    -f "${COMPOSE_FILE}" \
    "${compose_args[@]}"
}

main "$@"
