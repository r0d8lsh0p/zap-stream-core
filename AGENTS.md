# zap-stream-core (Shosho Fork)

## CRITICAL: No destructive Cloudflare actions without explicit user approval

**Never take any destructive action on Cloudflare resources without explicit user approval.** This includes deleting, modifying, or overwriting any resource via the Cloudflare API. Always ask first.

**Cloudflare Live Inputs are especially dangerous to delete.** Dev and staging environments share the same Cloudflare account. Deleting a live input is irreversible — there is no undo, no trash, no restore. The database stores `external_id` references to live inputs. If a live input is deleted from Cloudflare, every user and custom stream key that referenced it is permanently broken. The server can self-heal default user inputs on the next API call, but custom key inputs and any active streams are destroyed with no recovery path. If test data accumulates on Cloudflare, leave it.

---

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

> **READ `notes/deployment-model.md` FIRST.** The deployment model changed on
> 2026-10-02. Staging now deploys **any branch off `main`, unmodified** — nothing
> Railway needs lives in git any more. Production has **not** been migrated yet and
> still uses the older deploy-stack model described further down.

```
upstream/main ──(ff mirror)──► main     <-- pure mirror of v0l/zap-stream-core. NEVER commit here.
                                 │
                                 └── feat/x ──┬──► PR to v0l/zap-stream-core (upstream)
                                              │
                                              └──► point Railway STAGING at this branch
                                                   (deploys unmodified — no deploy stack)

dev/external ──► railway/external       <-- OLD model, PRODUCTION ONLY until migrated
```

### How to ship a change (new model — staging)

```bash
git switch -c feat/x main       # branch off main, the upstream mirror
# ...write code, commit...      # code only; nothing Railway-specific
git push -u origin feat/x       # then point Railway staging at feat/x in the dashboard
```

That is the whole flow. No cherry-picks, no shared branch, each change tested alone.
Raise the upstream PR from the same branch. Settings and config live in Railway — see
`notes/deployment-model.md` for the exact start command and variables.

### Production (old model, until migrated)

Production still deploys from `railway/external`, which carries the deploy stack
(`railway.toml`, baked `config.railway.external.yaml`, the fork migration file). To ship
to production today you still need the old two-step flow:

```bash
git switch dev/external && git cherry-pick feat/x
git push origin dev/external                     # (no longer deploys staging)
git push origin dev/external:railway/external    # production — user approval required
```

**THE RULE: never commit directly to `railway/external`.** It only ever receives a commit already validated elsewhere.

`dev/external` no longer auto-deploys staging (staging points at feature branches), so it
is now just the staging area for production promotions and the home of these docs. Keep
feature code OFF it — that is what caused the Sept 2026 collision.

### Branch rules

- **`main`** — a pure fast-forward **mirror of `upstream/main`**. Never commit here. Sync with `git fetch upstream && git branch -f main upstream/main`. Because it is a true mirror, any feature branch off `main` is upstream-submittable **by construction**.
- **`dev/external`** — `main` plus the deploy patch stack. Railway **staging** auto-deploys on push. Safe to test freely.
- **`railway/external`** — the same commit as `dev/external`. Railway **production** auto-deploys on push. **Never push without explicit user approval** — the single most dangerous action in this repo.

`integration/external` was **retired and deleted 2026-09-28**. It existed only because `main` used to be divergent from upstream. Do not recreate it.

### Upstream sync (periodic — the one exception)

Rebasing the stack onto a new `main` rewrites history, so this round *does* need force-pushes:

```bash
git fetch upstream && git branch -f main upstream/main
git rebase --onto main <old-main-sha> dev/external
# force-push dev/external -> validate on staging -> force-push railway/external to match
```

### Allowed divergences on `dev/external` and `railway/external`

`dev/external` = `main` + ONLY these categories. **Verify with `git diff main..dev/external --name-only`** — the only `.rs` file that may appear is `crates/zap-stream-external/src/main.rs`.

| Category | Files |
|----------|-------|
| Railway deployment config | `railway.toml`, `docs/deploy/config.railway.external.yaml`, `docs/RAILWAY.md` |
| Dockerfile config path | `crates/zap-stream-external/Dockerfile` — COPY line changed to use `config.railway.external.yaml` |
| Structured JSON logging | `crates/zap-stream-external/src/main.rs` (`LOG_FORMAT=json` support), `Cargo.toml` (`json` feature on tracing-subscriber) |
| Production data migration | `crates/zap-stream-db/migrations/20260224000000_migrate_cf_uid_to_external_id.sql` — already applied to the prod DB. **Cannot be deleted**: SQLx aborts at startup if a previously-applied migration file is missing. |
| Local dev config | `docs/deploy/config.local.external.yaml` |
| Local dev docker compose | `docs/deploy/docker-compose.override.yml` |
| Gitignore additions | `.gitignore` — entries for `.env`, local dev data |
| Agent documentation | `AGENTS.md`, `CLAUDE.md`, `notes/`, `.claude/skills/` |

**If a change does not fit a category above, it belongs upstream** — branch from `main` and PR to `v0l/zap-stream-core`. Ask before adding a new divergence category.

### CRITICAL: Production safety

**Pushing to `railway/external` on origin auto-deploys production.** Never push without explicit user approval.

## Operational warnings (not obvious from the code)

- **Metering is armed but dormant.** `bill_stream` charges per minute and **ends the stream at zero balance**, gated only on `endpoint.cost > 0`. All production ingest endpoints are currently `cost = 0`, so nothing is charged. Setting any endpoint's cost above zero starts metering and cut-offs **with no deploy** — it is a database lever, not a code change.
- **Upstream's GitHub Actions are disabled at the repo level, and that state is NOT in git.** `docker-build.yml` and `docker-pr.yml` publish to *upstream's* Docker Hub (`voidic`) using a `DOCKER_TOKEN` this fork does not have; `docker-build.yml` failed on every push to `main`. Both are `state=disabled_manually` via `gh workflow disable`. Re-enable with `gh workflow enable <id>`. The state is keyed by file path, so **if upstream renames or adds a workflow it arrives active and may start failing — check Actions after each `main` sync.**
- **There is no CI that builds or tests `zap-stream-external`.** Upstream's workflows only build `crates/zap-stream/Dockerfile` and `crates/n94-bridge/Dockerfile`, neither of which we deploy. Railway's "Wait for CI" therefore has nothing to gate on. Adding fork CI is feasible and cheap: the crate has **zero ffmpeg deps** and **zero `sqlx::query!` macros**, needs only `protobuf-compiler`, and all E2E tests are `#[ignore]`d so `cargo test -p zap-stream-external` runs just the unit tests.
- **Production's Cloudflare token lacks Account Alerting permission.** Startup logs `Failed to setup notification policy: 403 Authentication error` on every boot. Pre-existing and harmless while the webhook destination already exists — but production cannot self-heal its notification policy if it ever needs re-creating.
- **Deleting a stream publishes a NIP-09 request, which relays may ignore.** Observed 2026-09-28: only 2 of 4 public relays honoured it. The kind 30311 is replaced with `status=ended` regardless, so it stops advertising as live.

## Git ops workflow

1. Work on a feature branch **off `main`** (not off a deploy branch)
2. **Run cargo tests** — `cargo test -p zap-stream-external` and `cargo test -p zap-stream-db`
3. **Run local Docker E2E tests** — entirely on this machine, nothing deployed. See "Local dev test environment" below
4. **Cherry-pick onto `dev/external`** — a local commit only; deploys nothing until it is pushed
5. **`git push origin dev/external`** — **THIS DEPLOYS STAGING.** Needs the user's say-so; "test it" does not mean this
6. **Promote to `railway/external`** — `git push origin dev/external:railway/external` (user approval required)
7. **PR the feature branch to `v0l/zap-stream-core`** for upstreaming

### CRITICAL: "local" means local. It never means staging.

A request to "test locally" is satisfied entirely by steps 2-4 on this machine. It is **never** satisfied by pushing.

Staging deploys **only on `git push origin dev/external`** — Railway watches the *remote* branch. Committing or
cherry-picking onto a local `dev/external` deploys nothing; `git status -sb` showing `[ahead 1]` means staging has
**not** seen the change. Confirm with `git branch -r --contains <sha>`: empty output means it never left this machine.

**Local Docker E2E has to run from a checked-out `dev/external`** — `docs/deploy/docker-compose.override.yml` and
`docs/deploy/config.local.external.yaml` are deploy-branch-only files, so they do not exist on a feature branch. That
is the *only* reason to cherry-pick before testing, and it does not imply a push. Cherry-pick, run the harness, and
leave the branch unpushed until the user asks for staging.

### Local layout

A **single checkout**, no worktrees: `zap-stream-core-fork/`. Switch it between `main`, `dev/external` and `railway/external` as needed. It holds the canonical gitignored `docs/deploy/.env` — never delete or overwrite that file.

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

**This runs wholly on this machine and deploys nothing.** The stack is a local MariaDB plus a `zap-stream-external`
built from the working tree, publishing Nostr events to a local relay only. Two things do reach the outside world, by
design: Cloudflare Stream API calls (dev account), and the cloudflared tunnel that lets Cloudflare's webhooks back in.
Neither is staging. Staging is only ever touched by `git push origin dev/external`.

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
- **Never commit to `railway/external` or `main`** — `railway/external` only receives validated `dev/external` commits; `main` is a pure upstream mirror
- **Never `git add .` on `main`** — `main` mirrors upstream and has upstream's minimal `.gitignore`, so local files (`.env`, dev data) are NOT ignored there and could be exposed. The full `.gitignore` is a deploy-branch divergence.
- **Never `git add -f`** to bypass `.gitignore` without explicit user approval
- **Verify branch before any push**: `git branch --show-current`
- **Each branch has its own `.gitignore`** — files safe on one branch may be exposed on another

## Operations & Railway access

Read `notes/operating-procedures.md` for all operational procedures. Key procedures:

- **Railway logs and env vars**: Use the temp directory pattern with `railway link`. Project ID, service name, and examples are in the `railway-logs` skill (`.claude/skills/railway-logs/SKILL.md`) and in `notes/operating-procedures.md` procedure 1.
- **Nostr event operations**: Use `nak` with the server nsec and `a` tag for working with replaceable events. e.g. see procedure 6.
- **Staging smoke test**: Use a persistent test nsec (not throwaway), clean up test events afterwards. See procedure 7.

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
- `github-issues` — Issues live in `r0d8lsh0p/shosho-monorepo`, not this repo. **There is no GitHub project board**
- `vibe-kanban` — Task management workflow for agents
- `railway-logs` — Read production logs from Railway
- `skill-creator` — Create new skills for this repo

## GitHub issues

Issues are tracked in the **shosho-monorepo** GitHub repo (`r0d8lsh0p/shosho-monorepo`), not this repo. Do NOT run `gh repo set-default`. If you cannot find issues, ask the user.
