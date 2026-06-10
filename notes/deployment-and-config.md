# Deployment & Configuration Guide for zap-stream-external

## How the binary loads configuration

The binary uses the Rust `config` crate (`crates/zap-stream-external/src/main.rs`, lines 77-83):

```rust
Config::builder()
    .add_source(config::File::with_name("config.yaml"))         // 1. required file
    .add_source(config::Environment::with_prefix("APP").separator("__"))  // 2. env var overlay
    // debug builds only:
    .add_source(config::File::with_name("config.dev.yaml").required(false))  // 3. optional dev file
```

Loading order (later sources override earlier):
1. `config.yaml` in the working directory — **required, binary won't start without it**
2. `APP__*` environment variables — overlay on top (e.g. `APP__DATABASE`, `APP__NSEC`, `APP__CLOUDFLARE__TOKEN`)
3. `config.dev.yaml` — optional, only in debug builds

**Critical**: If a key has a value in `config.yaml`, the env var for that key is **ignored**. The `config` crate loads files first, then env vars, but file values take precedence over env vars for the same key. To allow env var override, the key must be **commented out** in the config file.

Source: `docs/deploy/config.railway.external.yaml` header comment explains this.

## Config files in this repo

### Tracked files

| File | Branch | Purpose |
|------|--------|---------|
| `crates/zap-stream-external/config.yaml` | All (from upstream) | Upstream maintainer's local dev defaults. Placeholder values (`kieran.lan`, dummy CF creds). Used by `cargo run` from the crate directory. |
| `docs/deploy/config.railway.external.yaml` | `dev/external`, `railway/external` | Production config baked into Docker image. Secrets are commented out so Railway env vars can override them. |
| `docs/deploy/config.local.external.yaml` | `dev/external`, `railway/external` | Local dev config. Points to local test relay (`ws://host.docker.internal:3334`). Secrets commented out so `.env` vars can override them. |
| `docs/deploy/compose-config.yaml` | All (from upstream) | Config template for the **old** `zap-stream` binary. Not used by `zap-stream-external`. |

### Untracked files (gitignored, contain secrets)

| File | Purpose |
|------|---------|
| `docs/deploy/.env` | Local dev secrets: CF token, nsec, tunnel URL, DB password. Loaded by `docker-compose.override.yml` via `env_file`. |
| `crates/zap-stream-external/tests/.env` | Cloudflare credentials for Rust E2E tests (optional). See `.env.example` for format. |

## How production deployment works (Railway)

1. Railway detects a push to `railway/external` on origin
2. Railway builds the Docker image using `crates/zap-stream-external/Dockerfile`
3. The Dockerfile copies `docs/deploy/config.railway.external.yaml` as `/app/config.yaml` into the image (line 35)
4. At runtime, Railway injects secrets as env vars (`APP__DATABASE`, `APP__NSEC`, `APP__CLOUDFLARE__TOKEN`, `APP__CLOUDFLARE__ACCOUNT_ID`, `APP__ENDPOINTS_PUBLIC_HOSTNAME`)
5. The binary loads `config.yaml` then overlays the env vars — secrets take effect because they are commented out in the config file
6. On startup, the binary auto-registers the Stream webhook with Cloudflare (`PUT /stream/webhook`)

**Staging** works identically but deploys from `dev/external`.

**Cloudflare notification policy** (one-time per account): The `live_input.connected` and `live_input.disconnected` events require a separate notification policy configured via the Cloudflare Alerting API. This is NOT auto-created on startup (see r0d8lsh0p/shosho-monorepo#824). Follow `docs/CLOUDFLARE_BACKEND.md` step 4 for setup. The API token needs both **Stream** and **Notifications** permissions.

**The Dockerfile differs between branches.** This is a direct edit, not a Docker override:
- `integration/external`: `COPY crates/zap-stream-external/config.yaml /app/config.yaml` (upstream default)
- `dev/external` / `railway/external`: `COPY docs/deploy/config.railway.external.yaml /app/config.yaml` (our production config)

When cherry-picking commits that modify the Dockerfile, watch for conflicts on this line.

## How local Docker testing works

Local dev testing uses `docs/deploy/docker-compose.override.yml` — a **self-contained** compose file (not merged with any base compose). It includes:
- MariaDB container with `zap_stream` database
- `zap-stream-external` container built from source
- `config.local.external.yaml` mounted as `/app/config.yaml` (replaces the baked-in railway config)
- Secrets loaded from `.env` via `env_file`

```bash
cd docs/deploy

# Start (builds from source)
docker compose -f docker-compose.override.yml up --build -d

# Logs
docker compose -f docker-compose.override.yml logs -f zap-stream-external

# Stop
docker compose -f docker-compose.override.yml down

# Reset database (removes named volume)
docker compose -f docker-compose.override.yml down -v
```

**Key differences from the base compose (`docker-compose.external.yaml`):**
- Self-contained — includes both db and service, no merge needed
- Builds from source instead of pulling `voidic/zap-stream-external:latest`
- Mounts `config.local.external.yaml` (local relay) instead of `config.yaml`
- Port mapping: `8090:8080` (avoids clash with port 8080 used by other services)
- No LND cert/macaroon volume mounts (not needed for LNURL payment backend)

**A cloudflared tunnel is required** for Cloudflare webhooks to reach the local service. Start with `nohup` so it persists:

```bash
nohup cloudflared tunnel --url http://localhost:8090 > /tmp/cloudflared-dev.log 2>&1 &
```

Update `APP__PUBLIC_URL` in `.env` with the tunnel URL, then restart the stack.

## How `cargo run` local testing works

When running the binary directly (not in Docker):

```bash
cd crates/zap-stream-external
cargo run
```

It loads `crates/zap-stream-external/config.yaml` (upstream's default with placeholder values). Override with env vars or create a `config.dev.yaml` alongside it (debug builds only, not tracked).

## How Rust E2E tests work

Tests live in `crates/zap-stream-external/tests/`. They require the Docker stack to be running (MariaDB on port 3306, API on port 8090).

```bash
ZS_API_PORT=8090 DB_ROOT_PASSWORD=devpass123 cargo test -p zap-stream-external -- --ignored --nocapture
```

See `crates/zap-stream-external/tests/TESTING_README.md` for the full testing playbook.

## Docker Compose files — old vs new binary

There are multiple docker-compose files. Do not confuse them:

| File | Binary | Purpose |
|------|--------|---------|
| `docker-compose.yaml` | Old `zap-stream` (monolithic) | Upstream compose, do not modify |
| `docker-compose.external.yaml` | New `zap-stream-external` | Base compose for standalone use (pulls published image) |
| `docker-compose.override.yml` | New `zap-stream-external` | **Local dev** — self-contained, builds from source, uses local config |

For local dev testing, use only `docker-compose.override.yml`. Do not merge it with other compose files.

## Anti-patterns — mistakes to avoid

### 1. Do not rename tracked upstream files

Files like `compose-config.yaml`, `docker-compose.yaml`, `docker-compose.external.yaml` exist on `integration/external` and upstream. Do not rename, delete, or restructure them.

### 2. Do not create config files that nothing references

Before creating a new config file, identify exactly what will load it and how. Check:
- Does the binary's `Config::builder()` reference this filename?
- Does a docker-compose volume mount reference this path?
- Does a Dockerfile `COPY` reference this path?

### 3. Do not confuse old binary config with new binary config

`compose-config.yaml` uses the old `overseer:` structure. The external binary uses a flat `Settings` struct — `database`, `nsec`, `cloudflare.token`, etc. They are completely different formats.

### 4. Understand the config precedence before making changes

The `config` crate's precedence means:
- If `config.yaml` has `nsec: "nsec1abc..."`, then `APP__NSEC=nsec1xyz...` is **ignored**
- To allow env var override, the key must be **commented out** in the YAML file
- This is why both `config.railway.external.yaml` and `config.local.external.yaml` have secrets commented out

Getting this wrong means the service silently uses the wrong credentials.

### 5. Do not edit the Dockerfile on integration/external to reference railway-specific files

The Dockerfile on `integration/external` must reference only upstream-compatible paths. Railway-specific changes (like the config file path) belong on `dev/external` and `railway/external` only.

### 6. NEVER delete a migration file that has been applied to production

SQLx's `migrate!().run()` calls `validate_applied_migrations()` on startup. It compares the embedded migration files against the `_sqlx_migrations` table in the database. If the database has a record of a migration that is not in the binary, SQLx returns `MigrateError::VersionMissing` and **the service crashes on startup**.

Once a migration has been applied to any database, its file must remain in the `migrations/` directory forever (or until the `_sqlx_migrations` row is manually deleted from that database).

### 7. Do not merge docker-compose files for local dev

The override file (`docker-compose.override.yml`) is self-contained. Running it with `-f docker-compose.external.yaml -f docker-compose.override.yml` causes port conflicts, phantom volume mounts, and confusing merge behavior. Use the override alone.
