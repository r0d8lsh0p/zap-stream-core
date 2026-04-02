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
| `crates/zap-stream-external/config.yaml` | Both (from upstream) | Upstream maintainer's local dev defaults. Placeholder values (`kieran.lan`, dummy CF creds). Used by `cargo run` from the crate directory. |
| `docs/deploy/config.railway.external.yaml` | `railway/external` only | Production config baked into Docker image. Secrets are commented out so Railway env vars can override them. |
| `docs/deploy/compose-config.yaml` | Both (from upstream) | Config template for the **old** `zap-stream` binary. Not used by `zap-stream-external`. |

### Untracked files (gitignored, contain secrets)

| File | Purpose |
|------|---------|
| `docs/deploy/config.local.external.yaml` | Local dev config for the external binary. Created by copying `config.railway.external.yaml` and filling in local values. |
| `crates/zap-stream-external/tests/.env` | Cloudflare credentials for Rust E2E tests. See `.env.example` for format. |

## How production deployment works (Railway)

1. Railway detects a push to `railway/external` on origin
2. Railway builds the Docker image using `crates/zap-stream-external/Dockerfile`
3. The Dockerfile copies `docs/deploy/config.railway.external.yaml` as `/app/config.yaml` into the image (line 35)
4. At runtime, Railway injects secrets as env vars (`APP__DATABASE`, `APP__NSEC`, `APP__CLOUDFLARE__TOKEN`, `APP__CLOUDFLARE__ACCOUNT_ID`)
5. The binary loads `config.yaml` then overlays the env vars — secrets take effect because they are commented out in the config file

**The Dockerfile differs between branches.** This is a direct edit, not a Docker override:
- `integration/external`: `COPY crates/zap-stream-external/config.yaml /app/config.yaml` (upstream default)
- `railway/external`: `COPY docs/deploy/config.railway.external.yaml /app/config.yaml` (our production config)

When cherry-picking commits that modify the Dockerfile, watch for conflicts on this line.

## How local Docker testing works

`docs/deploy/docker-compose.external.yaml` defines the local Docker stack:
- MariaDB container with `zap_stream` database
- `zap-stream-external` container, mounting `./config.yaml:/app/config.yaml`

The compose file expects `docs/deploy/config.yaml` to exist. This file does NOT exist by default — you must create it:

```bash
cd docs/deploy
cp config.railway.external.yaml config.yaml
# Edit config.yaml: fill in database URL, nsec, Cloudflare creds
# Use local MariaDB connection: mysql://root:${DB_ROOT_PASSWORD}@db/zap_stream
```

`docs/deploy/config.yaml` is gitignored to prevent committing secrets.

**Note**: `docker-compose.external.yaml` references `./init.sql` for MariaDB initialization. This file may not exist — the binary runs its own migrations via `db.migrate()` at startup, so it may not be needed.

## How `cargo run` local testing works

When running the binary directly (not in Docker):

```bash
cd crates/zap-stream-external
cargo run
```

It loads `crates/zap-stream-external/config.yaml` (upstream's default with placeholder values). Override with env vars or create a `config.dev.yaml` alongside it (debug builds only, not tracked).

## How Rust E2E tests work

Tests live in `crates/zap-stream-external/tests/`. They load credentials from `crates/zap-stream-external/tests/.env` (gitignored). Copy `.env.example` and fill in Cloudflare credentials.

```bash
cargo test -p zap-stream-external
```

## Docker Compose files — old vs new binary

There are two docker-compose files. Do not confuse them:

| File | Binary | Config it mounts |
|------|--------|-----------------|
| `docker-compose.yaml` | Old `zap-stream` (monolithic) | `./compose-config.yaml` |
| `docker-compose.external.yaml` | New `zap-stream-external` (Cloudflare) | `./config.yaml` |

The old binary's compose (`docker-compose.yaml`) and config template (`compose-config.yaml`) are tracked upstream. Do not delete or modify them.

## Anti-patterns — mistakes to avoid

### 1. Do not rename tracked upstream files

Files like `compose-config.yaml`, `docker-compose.yaml`, `docker-compose.external.yaml` exist on `integration/external` and upstream. Do not rename, delete, or restructure them. Create new local files alongside them instead.

### 2. Do not create config files that nothing references

Before creating a new config file, identify exactly what will load it and how. Check:
- Does the binary's `Config::builder()` reference this filename?
- Does a docker-compose volume mount reference this path?
- Does a Dockerfile `COPY` reference this path?

If nothing references it, don't create it.

### 3. Do not confuse old binary config with new binary config

`compose-config.yaml` and `compose-config.local.yaml` use the old `overseer:` structure with nested `cloudflare:` under it. The external binary uses a flat `Settings` struct — `database`, `nsec`, `cloudflare.token`, etc. They are completely different formats. Do not copy values between them without understanding the structure.

### 4. Do not conflate "not tracked" with "not used"

A file can be gitignored AND essential for local dev. `docs/deploy/config.yaml` must exist for local Docker testing but must never be committed. Its absence from git does not mean it's unnecessary.

### 5. Do not assert files are unused without checking all references

Before claiming a config file is unused, check:
- `Dockerfile` — `COPY` lines
- `docker-compose*.yaml` — volume mounts
- `main.rs` — `Config::builder()` sources
- `.gitignore` — may indicate the file is expected to exist locally

### 6. Do not delete local config files without preserving secrets

Files like `compose-config.local.yaml` contain live nsec keys and credentials. Before deleting, ensure the secrets are preserved somewhere accessible (another gitignored file, a password manager, etc.).

### 7. Understand the config precedence before making changes

The `config` crate's precedence means:
- If `config.yaml` has `nsec: "nsec1abc..."`, then `APP__NSEC=nsec1xyz...` is **ignored**
- To allow env var override, the key must be **commented out** in the YAML file
- This is why `config.railway.external.yaml` has secrets commented out — so Railway env vars work

Getting this wrong means production silently uses the wrong credentials.

### 8. Do not edit the Dockerfile on integration/external to reference railway-specific files

The Dockerfile on `integration/external` must reference only upstream-compatible paths. Railway-specific changes (like the config file path) belong on `railway/external` only, as a direct edit that diverges from integration.

### 9. NEVER delete a migration file that has been applied to production

SQLx's `migrate!().run()` calls `validate_applied_migrations()` on startup. It compares the embedded migration files against the `_sqlx_migrations` table in the database. If the database has a record of a migration that is not in the binary, SQLx returns `MigrateError::VersionMissing` and **the service crashes on startup**.

This means: once a migration has been applied to any database, its file must remain in the `migrations/` directory forever (or until the `_sqlx_migrations` row is manually deleted from that database).

The `set_ignore_missing(true)` escape hatch exists but is not used in this codebase.
