# zap-stream-core (Shosho Fork)

## What this is

A fork of [v0l/zap-stream-core](https://github.com/v0l/zap-stream-core) — a Rust backend for live streaming on Nostr. Our fork adds Cloudflare Stream as a pluggable backend via the `zap-stream-external` binary.

**Upstream repo**: `https://github.com/v0l/zap-stream-core.git` (remote: `upstream`)
**Our fork**: `https://github.com/r0d8lsh0p/zap-stream-core.git` (remote: `origin`)

## Crate structure

| Crate | Role | We modify? |
|-------|------|------------|
| `zap-stream-external` | Cloudflare Stream backend binary — **our primary workspace** | Yes |
| `zap-stream-db` | Shared database layer (MariaDB/MySQL) | Yes (when adding queries/models) |
| `zap-stream-api-common` | Shared API types and traits | Sometimes |
| `core`, `core-nostr`, `zap-stream` | Upstream core — self-hosted backend | Rarely, with care |
| `n94`, `n94-bridge`, `migration-tool` | Upstream utilities | No |

Key source files in `zap-stream-external`:
- `src/cloudflare/api.rs` — main Cloudflare integration (webhooks, polling, stream lifecycle)
- `src/cloudflare/client.rs` — Cloudflare HTTP API client
- `src/cloudflare/types.rs` — Cloudflare API response types
- `src/main.rs` — binary entrypoint and configuration

## Branch strategy

```
dev/external               <-- daily work, local testing
                               Railway staging auto-deploys from this branch
  |
  ├── PR + merge ────────► railway/external       <-- production deployment
  |                            Pushes to origin auto-deploy production
  |
  └── cherry-pick code ──► integration/external   <-- upstream PRs to v0l/zap-stream-core
                               NO Railway config, NO agent files, NO local dev tooling
```

### Branch rules

- **`dev/external`** — the daily working branch. Feature branches are created from here. Railway staging is connected to this branch; pushing deploys staging automatically.
- **`railway/external`** — production deployment branch. Receives PRs from `dev/external`. **Pushing to origin auto-deploys production.** Never push without explicit user approval.
- **`integration/external`** — upstream-submittable code only. No Railway config, no agent docs, no local dev tooling, no secrets. Every commit here should be suitable for a PR to `v0l/zap-stream-core`. Receives cherry-picked code-only commits from `dev/external`.

### Relationship between branches

`dev/external` and `railway/external` have **identical code**. The only difference is:
- `dev/external` pushes deploy **staging** (safe, test freely)
- `railway/external` pushes deploy **production** (dangerous, human approval required)

`integration/external` is a **strict subset** — it contains only code that is suitable for upstream. All Railway-specific files, local dev tooling, and agent docs are excluded.

### Allowed divergences on `dev/external` and `railway/external`

These branches contain every commit from `integration/external`, plus ONLY these categories of additions:

| Category | Files |
|----------|-------|
| Railway deployment config | `railway.toml`, `docs/deploy/config.railway.external.yaml`, `docs/RAILWAY.md` |
| Dockerfile config path | `crates/zap-stream-external/Dockerfile` — COPY line changed to use `config.railway.external.yaml` |
| Structured JSON logging | `crates/zap-stream-external/src/main.rs` (`LOG_FORMAT=json` support), `Cargo.toml` (`json` feature on tracing-subscriber) |
| Production data migration | `crates/zap-stream-db/migrations/20260224000000_migrate_cf_uid_to_external_id.sql` — one-time migration already applied to prod DB. **Cannot be deleted** because SQLx will crash on startup if a previously-applied migration file is missing. |
| Local dev config | `docs/deploy/config.local.external.yaml` — local dev config with test relay, secrets commented out |
| Local dev docker compose | `docs/deploy/docker-compose.override.yml` — self-contained compose for local dev testing |
| Local dev automation | `docs/deploy/dev.sh` — script to automate tunnel + docker + test workflow (planned) |
| Gitignore additions | `.gitignore` — entries for `.env`, local dev data |
| Agent documentation | `AGENTS.md`, `notes/` |

**If a divergence doesn't fit one of the categories above, it probably belongs on `integration/external` instead.** Ask before adding new divergences.

### CRITICAL: Production safety

**Pushing to `railway/external` on origin auto-deploys production.** Never push to this branch without explicit user approval. This is the single most dangerous action you can take in this repo.

## Git ops workflow

1. **Agent works on `dev/external`** (or a feature branch from it)
2. **Run cargo tests** — `cargo test -p zap-stream-external` and `cargo test -p zap-stream-db`
3. **Run local Docker E2E tests** — see "Local dev test environment" below
4. **Push `dev/external`** — auto-deploys staging, manual smoke test to verify
5. **PR from `dev/external` to `railway/external`** — deploys production (user approval required)
6. **Cherry-pick** code-only commits to `integration/external` for upstream PRs

### Local worktrees

| Directory | Branch | Purpose |
|-----------|--------|---------|
| `zap-stream-core-fork/` | `dev/external` | Daily workspace, local dev testing |
| `zap-stream-external/` | `integration/external` | Upstream PR clean room (legacy, may be removed) |

## Testing

### Cargo tests

```bash
# From repo root
cargo test -p zap-stream-external
cargo test -p zap-stream-db

# Run all tests
cargo test
```

### Local dev test environment

Local integration testing uses Docker Compose with a self-contained override file. The setup lives in `docs/deploy/`:

| File | Tracked? | Purpose |
|------|----------|---------|
| `docker-compose.override.yml` | Yes | Self-contained compose: MariaDB + external binary built from source |
| `config.local.external.yaml` | Yes | Local config: test relay (`ws://host.docker.internal:3334`), secrets commented out |
| `.env` | **No** (gitignored) | Local secrets: CF token, nsec, tunnel URL, DB password |
| `config.railway.external.yaml` | Yes | Production config baked into Docker image by Railway |

**Setup (one-time):**
1. Copy `.env` template and fill in real values (CF dev account credentials, dev nsec)
2. Ensure the local Nostr relay is running on port 3334

**Testing workflow:**
1. Start cloudflared tunnel: `nohup cloudflared tunnel --url http://localhost:8090 > /tmp/cloudflared-dev.log 2>&1 &`
2. Capture tunnel URL: `grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' /tmp/cloudflared-dev.log`
3. Update `APP__PUBLIC_URL` in `.env` with the tunnel URL
4. Start stack: `cd docs/deploy && docker compose -f docker-compose.override.yml up --build -d`
5. Check logs: `docker compose -f docker-compose.override.yml logs -f zap-stream-external`
6. Run E2E tests: `ZS_API_PORT=8090 DB_ROOT_PASSWORD=devpass123 cargo test -p zap-stream-external -- --ignored --nocapture`
7. Stop: `docker compose -f docker-compose.override.yml down`

**Key design points:**
- The override is self-contained — run it alone, do NOT merge with `docker-compose.external.yaml`
- `config.local.external.yaml` is mounted into the container, replacing the baked-in railway config
- Secrets are loaded from `.env` via `env_file` — never hardcoded in tracked files
- The local relay at `ws://host.docker.internal:3334` ensures Nostr events stay isolated from the public network
- Cloudflare API calls are external by design (testing a real CF integration)
- Port 8090 on host maps to 8080 in container (8080 is often occupied by other services)

See `crates/zap-stream-external/tests/TESTING_README.md` for the full E2E test playbook.

## Git safety

- **Never push `railway/external` without user approval** — auto-deploys production
- **Never `git add .` on `integration/external`** — its `.gitignore` is minimal (upstream-compatible), local files may be exposed
- **Never `git add -f`** to bypass `.gitignore` without explicit user approval
- **Verify branch before any push**: `git branch --show-current`
- **Each branch has its own `.gitignore`** — files safe on one branch may be exposed on another

## Deployment & Configuration

Read `notes/deployment-and-config.md` before touching any config files, Dockerfiles, or deployment workflows. It documents how the binary loads config, how production deployment works, how local Docker testing works, and anti-patterns to avoid.

## Cloudflare Stream API reference

The Cloudflare Stream API docs are maintained by Cloudflare:

- [Live Inputs API](https://developers.cloudflare.com/api/resources/stream/subresources/live_inputs/) — create, list, update, delete live inputs
- [Stream Videos API](https://developers.cloudflare.com/api/resources/stream/subresources/videos/) — manage recordings
- [Webhooks](https://developers.cloudflare.com/stream/manage-video-library/using-webhooks/) — `live_input.connected`, `live_input.disconnected`, video ready events
- [Stream Live](https://developers.cloudflare.com/stream/stream-live/) — overall live streaming guide

## Documentation index

### Must-read (safety)
- `notes/CRITICAL-GIT-STRATEGY.md` — Git safety, pre-commit checklist, anti-patterns (**READ FIRST**)
- `notes/deployment-and-config.md` — How config loading, Docker builds, and deployment work (**READ BEFORE touching config or Dockerfiles**)

### Operations
- `notes/operating-procedures.md` — Runbook for health checks, log reading, and administration

### Reference
- `docs/CLOUDFLARE_BACKEND.md` — Cloudflare Stream integration architecture, webhook setup
- `docs/RAILWAY.md` — Railway deployment setup
- `docs/API.md` — API reference
- `docs/ADMIN_API.md` — Admin API reference
- `crates/zap-stream-external/tests/TESTING_README.md` — E2E test harness and procedures

### Skills (`.claude/skills/`)
- `github-project-management` — Issues and project board are in `r0d8lsh0p/shosho-monorepo`, not this repo
- `vibe-kanban` — Task management workflow for agents
- `railway-logs` — Read production logs from Railway
- `skill-creator` — Create new skills for this repo

## GitHub issues

Issues are tracked in the **shosho-monorepo** GitHub repo (`r0d8lsh0p/shosho-monorepo`), not this repo. Do NOT run `gh repo set-default`. If you cannot find issues, ask the user.
