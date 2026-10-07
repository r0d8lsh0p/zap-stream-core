# zap-stream-core (Shosho Fork)

## CRITICAL: No destructive Cloudflare actions without explicit user approval

**Never take any destructive action on Cloudflare resources without explicit user approval.** This includes deleting, modifying, or overwriting any resource via the Cloudflare API. Always ask first.

**Cloudflare Live Inputs are especially dangerous to delete.** Dev and staging environments share the same Cloudflare account. Deleting a live input is irreversible — there is no undo, no trash, no restore. The database stores `external_id` references to live inputs. If a live input is deleted from Cloudflare, every user and custom stream key that referenced it is permanently broken. The server can self-heal default user inputs on the next API call, but custom key inputs and any active streams are destroyed with no recovery path. If test data accumulates on Cloudflare, leave it.

---

## 🛑 CRITICAL: Kind 30311 — the `p` host tag is the STREAMER, the signer is the SERVER. THIS IS CORRECT. NOT A BUG.

> # ⚠️ DO NOT FLAG THIS. DO NOT "FIX" THIS. DO NOT ASK THE USER ABOUT THIS.
>
> **Every kind 30311 live event is signed by the SERVER's key, and names the STREAMER in the `p` tag with the `host` role:**
>
> ```
> pubkey:  <SERVER pubkey>                              <- the server signs every 30311
> ["p", "<STREAMER pubkey>", "", "host"]                <- the streamer is the host
> ```
>
> **This is the required, mandatory, intended way ALL kind 30311 events work, in production and in every branch.**
> The event's author and its `host` are **supposed** to be different keys. That is how NIP-53 delegates hosting:
> the service publishes on the streamer's behalf, and clients attribute the stream to the `host`.
>
> - **It is NOT a bug.** If you think it is a bug, **you are wrong.**
> - **It is NOT a regression, a security issue, a spoofing risk, or a review finding.** Never list it as one.
> - **It NEVER needs the user's review.** Do not mention it in reviews, summaries, PR descriptions, or "things to check".
> - **Do not change it.** Do not move the streamer into `pubkey`, do not drop the `p` tag, do not change the `host` role.
>
> The user has had to explain this to agents repeatedly. Raising it again wastes their time.

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

> **READ `notes/deployment-model.md` FIRST.** Since 2026-10-05 **both staging and production deploy branches built on
> upstream `main`, unmodified.** Nothing Railway needs lives in git: settings and config live in Railway. There is no
> deploy stack and no cherry-picking.

```
upstream/main ──(ff mirror)──► main                <-- pure mirror of v0l/zap-stream-core. NEVER commit here.
                                 │
                                 └── feat/x ──┬──► point Railway STAGING at feat/x   (test it alone)
                                              ├──► PR into shosho-production          (PRODUCTION deploys on merge)
                                              └──► PR into v0l/zap-stream-core main   (upstream)

shosho-production   <-- what Railway PRODUCTION deploys. upstream main + our merged feature PRs.
```

### How to ship a change

```bash
git switch -c feat/x main       # branch off main, the upstream mirror
# ...write code, commit...      # code only; nothing Railway-specific
git push -u origin feat/x       # point Railway staging at feat/x in the dashboard, test it
gh pr create --repo r0d8lsh0p/zap-stream-core --base shosho-production --head feat/x   # production
gh pr create --repo v0l/zap-stream-core --base main --head r0d8lsh0p:feat/x            # upstream, later
```

The same branch feeds staging, production and upstream, so every commit has one hash everywhere.

### Rules that keep production and upstream in step

- **Merge PRs into `shosho-production` with a merge commit — never squash or rebase-merge.** Production then carries the
  same commits as the upstream PR, and syncing upstream later is clean. A squash creates a different commit with the
  same content, which duplicates or conflicts on the next sync.
- **Never force-push a branch with an open PR.** Review fixes go on as new commits.
- **Sync production with upstream by merging, not rebasing:** merge `upstream/main` into `shosho-production` (via a PR).
  Once upstream has merged one of our PRs, that merge brings nothing new for it.
- **Merging into `shosho-production` deploys production.** It needs the user's explicit approval. Never push to it
  directly.

### Branch rules

- **`main`** — a pure fast-forward **mirror of `upstream/main`**. Never commit here. Sync with
  `git fetch upstream && git branch -f main upstream/main`. Any branch off `main` is upstream-submittable **by
  construction**.
- **`shosho-production`** — Railway **production** deploys it. Changes arrive only by merged PR, with user approval.
- **`dev/external`** — **docs only**: `AGENTS.md`, `notes/`, `.claude/skills/` and the local test harness. Deploys
  nothing. Never put feature code on it.
- **`railway/external`** — **retired 2026-10-05**, kept for rollback only (see `notes/deployment-model.md`). Deploys
  nothing. Do not push to it.

`integration/external` was **retired and deleted 2026-09-28**. Do not recreate it.

## Operational warnings (not obvious from the code)

- **Metering is armed but dormant.** `bill_stream` charges per minute and **ends the stream at zero balance**, gated only on `endpoint.cost > 0`. All production ingest endpoints are currently `cost = 0`, so nothing is charged. Setting any endpoint's cost above zero starts metering and cut-offs **with no deploy** — it is a database lever, not a code change.
- **Upstream's GitHub Actions are disabled at the repo level, and that state is NOT in git.** `docker-build.yml` and `docker-pr.yml` publish to *upstream's* Docker Hub (`voidic`) using a `DOCKER_TOKEN` this fork does not have; `docker-build.yml` failed on every push to `main`. Both are `state=disabled_manually` via `gh workflow disable`. Re-enable with `gh workflow enable <id>`. The state is keyed by file path, so **if upstream renames or adds a workflow it arrives active and may start failing — check Actions after each `main` sync.**
- **There is no CI that builds or tests `zap-stream-external`.** Upstream's workflows only build `crates/zap-stream/Dockerfile` and `crates/n94-bridge/Dockerfile`, neither of which we deploy. Railway's "Wait for CI" therefore has nothing to gate on. Adding fork CI is feasible and cheap: the crate has **zero ffmpeg deps** and **zero `sqlx::query!` macros**, needs only `protobuf-compiler`, and all E2E tests are `#[ignore]`d so `cargo test -p zap-stream-external` runs just the unit tests.
- **Production's Cloudflare token lacks Account Alerting permission.** Startup logs `Failed to setup notification policy: 403 Authentication error` on every boot. Pre-existing and harmless while the webhook destination already exists — but production cannot self-heal its notification policy if it ever needs re-creating.
- **Deleting a stream publishes a NIP-09 request, which relays may ignore.** Observed 2026-09-28: only 2 of 4 public relays honoured it. Nothing else changes: the stream row, its stored event and any custom key are left as they are, so on relays that ignore the request the event stays visible.
- **The stored `event` on a stream is upstream's last-built event, not a record of what is on the network.** Core stores the ended event before publishing it and keeps it if the publish fails, and delete keeps it after the relays delete it.
- **Local E2E and staging share one Cloudflare webhook.** The dev Cloudflare account allows one Stream webhook URL and one notification destination. Whichever of the local harness and staging booted last receives `live_input.*` webhooks; the other gets none, and E2E tests time out waiting for "connected". Redeploy staging after local testing to give it back.

## Git ops workflow

1. Work on a feature branch **off `main`**
2. **Run cargo tests** — `cargo test --workspace`
3. **Run local Docker E2E tests** — entirely on this machine, nothing deployed. See "Local dev test environment" below
4. **Push the feature branch and point Railway staging at it** — needs the user's say-so
5. **PR it into `shosho-production`** — merging deploys production (user approval required; merge commit, not squash)
6. **PR the same branch to `v0l/zap-stream-core`** for upstreaming

### CRITICAL: "local" means local. It never means staging.

A request to "test locally" is satisfied entirely by steps 2-3 on this machine. It is **never** satisfied by pushing.

Pushing a branch deploys nothing by itself. Staging deploys whichever branch Railway is pointed at, so pushing *that*
branch redeploys staging. Check which branch staging is on before pushing to it.

### Local layout

A **single checkout**, no worktrees: `zap-stream-core-fork/`. It holds the canonical gitignored `docs/deploy/.env` —
never delete or overwrite that file.

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
Neither is staging.

Local integration testing uses Docker Compose with a self-contained override file. The setup lives in `docs/deploy/`:

| File | Tracked? | Purpose |
|------|----------|---------|
| `docker-compose.override.yml` | Yes | Self-contained compose: MariaDB + external binary built from source |
| `config.local.external.yaml` | Yes | Local config: test relay (`ws://host.docker.internal:3334`), secrets commented out |
| `.env` | **No** (gitignored) | Local secrets: CF token, nsec, tunnel URL, DB password |
| `config.railway.external.yaml` | Yes | Legacy: was baked into the image. Railway now takes config from `APP_CONFIG_YAML` |

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

**Testing a feature branch.** The harness files above exist only on `dev/external`, and a feature branch off `main`
must be tested without them in its tree. Copy them out of git into `/tmp` and build the checked-out feature branch:

```bash
mkdir -p /tmp/zs-harness
git show origin/dev/external:docs/deploy/config.local.external.yaml > /tmp/zs-harness/config.local.external.yaml
# write /tmp/zs-harness/docker-compose.yml from docs/deploy/docker-compose.override.yml on dev/external, with
#   build.context = <absolute repo path>, the config volume = /tmp/zs-harness/config.local.external.yaml and
#   env_file = <absolute repo path>/docs/deploy/.env
set -a && . docs/deploy/.env && set +a
docker compose -p zsfeat -f /tmp/zs-harness/docker-compose.yml up --build -d    # own project = fresh database
# ... run the E2E tests as above ...
docker compose -p zsfeat -f /tmp/zs-harness/docker-compose.yml down -v          # -v drops the test database
```

This tests exactly the feature branch, with nothing from `dev/external` in the build. `/tmp` is cleared on reboot, so
recreate the files each session.

See `crates/zap-stream-external/tests/TESTING_README.md` for the full E2E test playbook.

## Git safety

- **Never push to `shosho-production`** — it auto-deploys production and only receives merged PRs, with user approval
- **Never commit to `main`** — it is a pure upstream mirror
- **Never force-push a branch with an open PR**
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
