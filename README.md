# Sequoia Map

[![Rust](https://img.shields.io/badge/Rust-2024_Edition-b7410e?style=flat-square&logo=rust)](https://www.rust-lang.org/)
[![Leptos](https://img.shields.io/badge/Leptos-0.7-ef3939?style=flat-square)](https://leptos.dev/)
[![wgpu](https://img.shields.io/badge/wgpu-24.0-4b8bbe?style=flat-square)](https://wgpu.rs/)
[![Axum](https://img.shields.io/badge/Axum-0.8-222222?style=flat-square)](https://github.com/tokio-rs/axum)
[![WebAssembly](https://img.shields.io/badge/WebAssembly-654ff0?style=flat-square&logo=webassembly&logoColor=white)](https://webassembly.org/)
[![Tailwind CSS](https://img.shields.io/badge/Tailwind_CSS-06b6d4?style=flat-square&logo=tailwindcss&logoColor=white)](https://tailwindcss.com/)
[![PostgreSQL](https://img.shields.io/badge/PostgreSQL-17-4169e1?style=flat-square&logo=postgresql&logoColor=white)](https://www.postgresql.org/)
[![Docker](https://img.shields.io/badge/Docker-2496ed?style=flat-square&logo=docker&logoColor=white)](https://www.docker.com/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow?style=flat-square)](LICENSE)

A [Wynncraft](https://wynncraft.com/) guild territory map built entirely in Rust: server, client, and shared types, compiled to WebAssembly & GPU Accelerated *for reasons!!*. 


## Stack

| Layer | Crates |
|-------|-------------|
| **Server** | Axum 0.8, Tokio, SQLx 0.8, DashMap, Tower-HTTP |
| **Client** | Leptos 0.7 (CSR &rarr; WASM), wgpu 24.0 (WebGL), Tailwind CSS |
| **Shared** | Serde, Chrono, crc32fast |

## Quick Start

The comments in combination with the README.md should be enough for anyone to pick this up.

### Prerequisites

- [Rust](https://rustup.rs/) 1.86+
- `wasm32-unknown-unknown` target | `rustup target add wasm32-unknown-unknown`
- [Trunk](https://trunkrs.dev/) | `cargo install trunk --locked`
- A running PostgreSQL instance

### Development

```bash
# Start the client dev server 
cd client && trunk serve

# In another terminal, start the backend
DATABASE_URL="postgres://user:pass@localhost:5432/sequoia" cargo run -p sequoia-server
```

If `trunk serve` exits immediately with a `--no-color` parsing error, run it as:

```bash
cd client && NO_COLOR=true trunk serve
```

### Development (Docker Hot Reload)

Run the full dev stack (Postgres + Rust server hot reload + Trunk hot reload):

```bash
POSTGRES_PASSWORD=changeme docker compose -f docker-compose.dev.yml up --build
```

- Client dev server: `http://localhost:8080`
- Server API: `http://localhost:3000/api/...`
- Postgres (optional host access): `localhost:55432` (override with `POSTGRES_PORT`)
- Server reload: `cargo watch` (watches `server/` and `shared/`)
- Client reload: Trunk watch/rebuild in `client/`

### Docker Compose

```bash
POSTGRES_PASSWORD=changeme docker compose up --build
```

The app will be available at `http://localhost:3000`.
Compose now also runs:
- `restart: unless-stopped` for long-running services
- default Docker log rotation (`json-file`, configurable size/count)
- automatic PostgreSQL backups to a dedicated `pgbackups` volume

For faster repeat builds/deploys (especially on Coolify/VPS), use BuildKit cache and avoid
rebuilding unless needed:

```bash
# First build (or after Dockerfile/dependency changes)
DOCKER_BUILDKIT=1 COMPOSE_DOCKER_CLI_BUILD=1 docker compose build
docker compose up -d

# Normal restart without rebuilding
docker compose up -d --no-build
```

## Architecture

Three-crate Cargo workspace:

```
sequoia-map/
  shared/   — Territory, tower, and event types shared between server & client
  server/   — Axum API server, Wynncraft poller, SSE broadcaster, PostgreSQL persistence
  client/   — Leptos CSR app compiled to WASM, wgpu canvas renderer, sidebar UI
```

## Font Renderer Modes

Map label rendering now has four modes in `Settings -> Font -> Font Renderer`:

- `Auto` (default): uses Firefox behavior on Firefox and Classic behavior elsewhere.
- `Classic`: always uses the stable Canvas2D classic layout.
- `Dynamic`: keeps the previous Auto behavior (GPU static labels on Firefox mobile, dynamic Canvas2D layout otherwise).
- `GPU`: enables the experimental full GPU map-overlay path (static labels + dynamic map text + resource icons).

Notes:

- `GPU` mode initializes dual glyph atlases (fill + halo) and an icon atlas lazily.
- `Auto`/`Classic` behavior remains unchanged; full-stack GPU rendering is gated behind `GPU` mode only.
- If GPU text/icon resources fail to initialize, map overlays fall back to Canvas2D rendering.


## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | *(required)* |
| `RUST_LOG` | Tracing filter directive | `info` |
| `DB_MAX_CONNECTIONS` | SQLx PostgreSQL pool max connections | `10` |
| `SSE_BROADCAST_BUFFER` | In-memory SSE broadcast channel capacity | `256` |
| `SEQ_LIVE_HANDOFF_V1` | Enable sequence-aware live-state handoff | `true` |
| `GUILDS_ONLINE_CACHE_TTL_SECS` | Cache freshness threshold used by `/api/guilds/online` | `120` |
| `GUILDS_ONLINE_MAX_CONCURRENCY` | Max concurrent upstream guild fetches in `/api/guilds/online` | `8` |
| `DOCKER_LOG_MAX_SIZE` | Docker log max size before rotation (Compose) | `10m` |
| `DOCKER_LOG_MAX_FILE` | Docker log file count to retain (Compose) | `5` |
| `BACKUP_INTERVAL_HOURS` | Interval between automatic PostgreSQL backups | `6` |
| `BACKUP_RETENTION_DAYS` | Days to keep automatic PostgreSQL backups | `14` |

## PostgreSQL Backups

Automatic backups are created by the `postgres-backup` service and stored in the `pgbackups` volume.

Utility scripts:

```bash
# List backups currently stored in /backups
./ops/backup/list_backups.sh

# Trigger an immediate backup
./ops/backup/backup_now.sh

# Restore latest backup into current DB
./ops/backup/restore_backup.sh

# Restore a specific backup file path from /backups
./ops/backup/restore_backup.sh /backups/sequoia_20260224T230000Z.sql.gz
```

Important: backups are generated with `--clean --if-exists`, so restore will drop/recreate dumped objects in the current database. It does not drop the database itself.

## Monitoring And Alerting

- Health endpoint: `/api/health`
- Prometheus endpoint: `/api/metrics`
- Alert rules file: `ops/prometheus/alerts/sequoia-map-alerts.yml`

Prometheus metrics exposed by `/api/metrics`:

| Metric | Type | Meaning |
|--------|------|---------|
| `sequoia_territories` | gauge | Current number of territories in live snapshot |
| `sequoia_guild_cache_size` | gauge | Current guild cache size |
| `sequoia_history_available` | gauge (0/1) | Whether history storage is available |
| `sequoia_seq_live_handoff_v1_enabled` | gauge (0/1) | Whether seq handoff mode is enabled |
| `sequoia_live_state_requests_total` | counter | Total `/api/live/state` requests |
| `sequoia_persist_failures_total` | counter | Total update persistence failures |
| `sequoia_dropped_update_events_total` | counter | Total updates dropped before broadcast |
| `sequoia_persisted_update_events_total` | counter | Total updates persisted before broadcast |
| `sequoia_guilds_online_requests_total` | counter | Total `/api/guilds/online` requests |
| `sequoia_guilds_online_cache_hits_total` | counter | Total guild rows served from cache by `/api/guilds/online` |
| `sequoia_guilds_online_cache_misses_total` | counter | Total guild rows requiring upstream fetch in `/api/guilds/online` |
| `sequoia_guilds_online_upstream_errors_total` | counter | Total upstream failures while serving `/api/guilds/online` |

Predefined alerts in `ops/prometheus/alerts/sequoia-map-alerts.yml`:

- `SequoiaMapTargetDown`
- `SequoiaMapMetricsMissing`
- `SequoiaMapPersistFailures`
- `SequoiaMapDroppedUpdates`
- `SequoiaMapHistoryUnavailable`
- `SequoiaMapSeqLiveHandoffDisabled`
- `SequoiaMapLiveStateRequestSpike`

Coolify/VPS deployment notes:

- Configure health checks against `/api/health`.
- Ensure Prometheus can scrape `/api/metrics`
- Mount or sync `ops/prometheus/alerts/sequoia-map-alerts.yml` into your Prometheus rules directory.
- Tune alert thresholds (`for:` windows and request-rate thresholds) to match production traffic.

## CI And Integration Tests

- GitHub Actions workflow: `.github/workflows/ci.yml`
- CI provisions PostgreSQL (`postgres:17-alpine`) and runs server/client checks plus server tests.
- Included a -Postgres integration test that verifies:
  - poller update persistence into `territory_events`
  - `/api/history/bounds`
  - `/api/history/events`
  - `/api/history/sr-samples`
  - `/api/history/at`
- Route integration tests also cover:
  - invalid history query params returning `400`
  - history pagination via `after_seq`
  - `/api/health` and `/api/metrics` response contract shape

Run locally with a real database:

```bash
DATABASE_URL="postgres://postgres:postgres@localhost:5432/sequoia" \
  cargo test -p sequoia-server -- --nocapture
```

## Project Structure

```
.
├── Cargo.toml            # Workspace root
├── Dockerfile            # Multi-stage build (client WASM + server binary)
├── docker-compose.yml    # Server + PostgreSQL 17
├── client/
│   ├── index.html        # Trunk entry point
│   ├── input.css         # Tailwind CSS input
│   ├── Trunk.toml
│   └── src/
│       ├── main.rs       # WASM entry point
│       ├── app.rs        # Root Leptos component
│       ├── canvas.rs     # wgpu canvas & territory rendering
│       ├── sidebar.rs    # Territory list & details panel
│       ├── history.rs    # Historical data fetching
│       ├── playback.rs   # Timeline playback controls
│       ├── timeline.rs   # Timeline scrub bar
│       ├── minimap.rs    # Minimap overlay
│       ├── tower.rs      # Tower info rendering
│       ├── sse.rs        # SSE client connection
│       └── ...
├── server/
│   ├── src/
│   │   ├── main.rs       # Axum server bootstrap
│   │   ├── state.rs      # Shared app state (DashMap + broadcast)
│   │   ├── config.rs     # Environment configuration
│   │   ├── routes/       # HTTP & SSE route handlers
│   │   └── services/     # Wynncraft poller, DB persistence
│   └── ...
└── shared/
    └── src/
        ├── lib.rs        # Re-exports
        ├── territory.rs  # Territory types
        ├── tower.rs      # Tower & resource types
        ├── colors.rs     # Guild color generation
        └── ...
```

## License

[MIT](LICENSE)
