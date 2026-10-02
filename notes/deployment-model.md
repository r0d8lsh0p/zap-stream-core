# Deployment model

**Status: staging migrated 2026-10-02. PRODUCTION IS STILL ON THE OLD MODEL.**
Until production is migrated, both models are live at once — see "Current state".

## Why this changed

The old model required every change to exist twice: once as `main` + change (for the
upstream PR) and once as `main` + deploy stack + change (to deploy). That forced
cherry-picks, gave the same change two different commit hashes, and funnelled
everything through a shared `dev/external` where unrelated changes piled up. Two
changes landed there together in Sept 2026 and neither could be tested on its own.

The new model removes everything Railway needs from git, so **any branch off `main`
deploys unmodified**. That restores the normal workflow: push a branch, raise a PR,
point Railway at the branch, test it in isolation.

## What moved out of the repo

| Thing | Old model | New model |
|---|---|---|
| `railway.toml` | Committed on deploy branches; Railway read builder, Dockerfile path, start command and restart policy from it. | Not in git. Those values live in **Railway service settings**. |
| Dockerfile change | Deploy branches carried a modified Dockerfile with `COPY docs/deploy/config.railway.external.yaml /app/config.yaml`, baking config into the image. | **Upstream's Dockerfile, unmodified.** |
| `config.railway.external.yaml` | Config committed to the repo and baked in at build time. Changing a URL meant a commit and a rebuild. | Held in the Railway variable **`APP_CONFIG_YAML`**; the start command writes it to `/app/config.yaml` at boot. Editable in the dashboard, no rebuild. |
| Fork migration `20260224000000_migrate_cf_uid_to_external_id.sql` | Carried forever on deploy branches. Could not be removed: the DB recorded it as applied, and SQLx aborts at startup if an applied migration's file is missing. | **Its row was deleted from `_sqlx_migrations`**, so SQLx no longer expects the file. The data fix had already been applied and the `UPDATE` is idempotent. |
| JSON logging (`main.rs`, `Cargo.toml`) | Deploy-branch divergence; `LOG_FORMAT=json` gave structured logs. | **Not carried — currently lost.** `LOG_FORMAT=json` is still set on staging but nothing reads it, so staging logs are plain text. Upstream it or accept plain text. |
| Agent docs (`AGENTS.md`, `CLAUDE.md`, `notes/`, `.claude/skills/`) | Deploy branches only. | **Unsolved.** Still deploy-branch only. A branch off `main` sees *upstream's* `AGENTS.md`, which describes a different workflow. Memory partly covers this. |
| `.gitignore` `**/.env` rule | Deploy branches only. | **Per-clone workaround**: `.git/info/exclude` plus a `gitleaks` pre-commit hook. Not in the repo; does not survive a fresh clone. |

Of the old 20-file deploy stack, **14 never reached the running service** (docs, notes,
skills, local harness). Only the first four rows above mattered for deployment.

## The Railway settings that replace it

Service: `ZS Core with CF Stream`.

- **Builder:** `DOCKERFILE`
- **Dockerfile path:** `crates/zap-stream-external/Dockerfile` — no leading slash
- **Custom Start Command:**
  ```
  sh -c 'set -e; printf "%s" "$APP_CONFIG_YAML" > /app/config.yaml; exec /app/bin/zap-stream-external'
  ```
- **Pre-deploy Command:** must be EMPTY (see gotchas)
- **Restart policy:** `ON_FAILURE`, max 10 retries
- **Variables:** `APP_CONFIG_YAML` (full config YAML) plus the existing secrets as
  discrete vars — `APP__DATABASE`, `APP__NSEC`, `APP__CLOUDFLARE__TOKEN`,
  `APP__CLOUDFLARE__ACCOUNT_ID`, `APP__PUBLIC_URL`. Env vars override the file, so
  secrets stay out of `APP_CONFIG_YAML`.

`payments` **must** come from `APP_CONFIG_YAML`, not env vars: `PaymentBackend` is an
externally-tagged pick-one enum (`lnd`/`bitvora`/`nwc`/`lnurl`) and upstream's baked
config sets `payments.lnd`. Adding `APP__PAYMENTS__LNURL__ADDRESS` on top merges into a
two-key map and fails to deserialize. Replacing the whole file is the only way to
switch backend.

`APP__RELAYS` must NOT be set on a branch without the list-parsing code (see
`env_source()` on `feat/env-config-lists`). Plain `config::Environment` cannot parse a
comma-separated string into `Vec<String>` and the service crash-loops. Put relays in
`APP_CONFIG_YAML` instead.

## Railway capabilities this relies on

1. **Config-as-code overrides the dashboard.** If `railway.toml` exists in the deployed
   branch it wins, and those dashboard fields are greyed out. Remove the file and the
   dashboard settings apply. Consequence: change the source branch FIRST, then edit
   settings — the fields only unlock once a branch without `railway.toml` is deployed.
2. **Custom Start Command replaces the image ENTRYPOINT.** You can run arbitrary shell
   before the binary, so files can be created at boot rather than baked at build time.
   Works because the runtime image (`debian:trixie-slim`) has a shell at `/usr/bin/sh`.
3. **Variables can hold multi-line content**, so a whole YAML file fits in one variable.
4. **Pre-deploy Command is a separate one-shot step that must exit.**
5. **Service settings persist independently of the repo** and can be silently wrong
   while `railway.toml` masks them.

## Gotchas that cost us time

- **Start command in the Pre-deploy field.** The command ends in `exec <server>`, which
  never exits, so Railway sat at "Running pre-deploy command..." indefinitely. It hangs
  silently rather than failing. The service was actually running correctly, just in the
  wrong lifecycle phase.
- **Stale stored Dockerfile path.** The service had `/crates/zap-stream/Dockerfile`
  stored — wrong crate, leading slash — masked by `railway.toml` for months. Removing
  the toml exposed it and the first build failed on `--mount=type=cache` flags from
  upstream's *other* Dockerfile. **Check prod's stored settings BEFORE removing its
  `railway.toml`.**
- **First build on a new branch is slow** (~10 min): cold cache plus the ~800 MB
  `voidic/rust-ffmpeg` base image. Subsequent builds are fast. The FFmpeg C toolchain is
  NOT compiled (`cargo tree -p zap-stream-external | grep ffmpeg` → 0), so a 45-minute
  build means something is wrong with feature gating.

## Current state

| | Branch | Model |
|---|---|---|
| Staging | `feat/planned-shows` (any feature branch) | **New** — migrated 2026-10-02 |
| Production | `railway/external` | **Old** — `railway.toml` + baked config + migration file |

Verified on staging: `Using LNURL payment backend: rb@rodbishop.nz` (injected config
replaced upstream's `lnd`), all 4 relays from `APP_CONFIG_YAML` connected, zero config
or migration errors.

## Migrating production (not yet done)

1. Record prod's current stored service settings, especially Dockerfile path — expect it
   to be stale and wrong.
2. Delete the fork migration row from the prod DB. Capture the row first; restore SQL
   pattern is in `/tmp/zsx-staging-migration-restore.sql` from the staging run.
3. Set `APP_CONFIG_YAML` on prod; delete `APP__RELAYS` if present.
4. Point the prod service at the branch; expect the first deploy to fail; then set
   Dockerfile path and Custom Start Command, clear Pre-deploy, redeploy.
5. Only after prod is migrated can `dev/external` and `railway/external` be retired.

Rollback at any point: restore the migration row, re-point the service at
`railway/external`, and the old model works again unchanged.
