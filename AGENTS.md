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
upstream/main (read-only, fetch only)
  |
  v
integration/external   <-- clean commits for upstream submission
  |                        NO Railway config, NO agent files
  |
  v  (cherry-pick)
railway/external       <-- production deployment branch
                           Railway config (railway.toml) + AGENTS.md
                           Pushes to origin auto-deploy production
```

### Branch rules

- **`integration/external`** — upstream-submittable code only. No local config, no agent docs, no secrets. Every commit here should be suitable for a PR to `v0l/zap-stream-core`.
- **`railway/external`** — `integration/external` + Railway-specific commits on top. Must always be a clean superset of `integration/external`.
- **Feature branches / worktrees** — branch from `integration/external` for new work.

### Allowed divergences on `railway/external`

`railway/external` must contain every commit from `integration/external` (via cherry-pick), plus ONLY these categories of railway-specific additions:

| Category | Files |
|----------|-------|
| Railway deployment config | `railway.toml`, `docs/deploy/config.railway.external.yaml`, `docs/RAILWAY.md` |
| Dockerfile config path | `crates/zap-stream-external/Dockerfile` — COPY line changed to use `config.railway.external.yaml` |
| Structured JSON logging | `crates/zap-stream-external/src/main.rs` (`LOG_FORMAT=json` support), `Cargo.toml` (`json` feature on tracing-subscriber) |
| Production data migration | `crates/zap-stream-db/migrations/20260224000000_migrate_cf_uid_to_external_id.sql` — one-time migration already applied to prod DB. **Cannot be deleted** because SQLx will crash on startup if a previously-applied migration file is missing. |
| Agent documentation | `AGENTS.md`, `notes/` |

**If a commit exists on `integration/external`, it MUST also be on `railway/external`.** Missing cherry-picks mean production is running different code than what's in the upstream PR.

**If a divergence doesn't fit one of the categories above, it probably belongs on `integration/external` instead.** Ask before adding new railway-only code changes.

### CRITICAL: Production safety

**Pushing to `railway/external` on origin auto-deploys production.** Never push to this branch without explicit user approval. This is the single most dangerous action you can take in this repo.

## Git ops workflow

1. **Agent works on a worktree** branched from `integration/external`
2. **Run cargo tests** — `cargo test -p zap-stream-external` and `cargo test -p zap-stream-db`
3. **User reviews and tests** the worktree changes
4. **Squash-merge** the worktree branch into `integration/external`
5. **Cherry-pick** that single squash commit into `railway/external`
6. **Push `integration/external`** to the open PR on the upstream repo (user approval required)
7. **Push `railway/external`** to deploy production (user approval required)

### Creating a worktree

```bash
cd /Users/visitor/projects/zap-stream/zap-stream-core
git worktree add ../zap-stream-wt-<issue>  integration/external -b feature/<issue>-<description>
```

### Merging back

```bash
# Squash into integration/external
git checkout integration/external
git merge --squash feature/<issue>-<description>
git commit -m "Description of change (closes #NNN)"

# Cherry-pick into railway/external
git checkout railway/external
git cherry-pick <squash-commit-hash>
```

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

A persistent worktree at `/Users/visitor/projects/zap-stream/zap-stream-external` is used for local integration testing with Docker. This worktree has local dev configuration — **never commit local config changes from this worktree back to `integration/external`**.

See `notes/CRITICAL-GIT-STRATEGY.md` for the full pre-commit checklist including Docker integration tests and stream start/end verification.

## Git safety

Read `notes/CRITICAL-GIT-STRATEGY.md` before any git operations. Key points:

- **Each branch has its own `.gitignore`** — files safe on one branch may be exposed on another
- **Never `git add .` on upstream-based branches** — our `.gitignore` additions may not exist
- **Never `git add -f`** to bypass `.gitignore` without explicit user approval
- **Never push upstream-based branches** — keep them local only
- **Verify branch before any push**: `git branch --show-current`

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
