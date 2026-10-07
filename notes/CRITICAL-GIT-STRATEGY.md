# CRITICAL: Git & Safety Strategy for zap-stream-core

**READ THIS BEFORE ANY GIT OPERATIONS OR COMMITS**

> **UPDATED 2026-10-07.** Production moved to the new model on 2026-10-05 and deploys `shosho-production`.
> `railway/external` is retired (rollback only), `dev/external` deploys nothing, and `integration/external` was
> deleted on 2026-09-28. See `notes/deployment-model.md`.

This document consolidates hard-won lessons from real incidents. Every rule exists because something went wrong.

---

## Branch Structure

```
upstream/main ──► main            <-- pure mirror. NEVER commit here.
                    └── feat/x    <-- all code. Point staging at it; PR it into shosho-production and upstream.

shosho-production                 <-- PRODUCTION deploys this. Changes arrive only by merged PR.
dev/external                      <-- docs and local test harness only. Deploys nothing.
railway/external                  <-- retired 2026-10-05, rollback only. Deploys nothing.
```

### Branch Rules

| Branch | Purpose | Push to origin? |
|--------|---------|-----------------|
| `main` | Mirror of `upstream/main` | **Never** commit or push your own work |
| `feat/*` off `main` | All code changes; staging is pointed here to test | Yes. It redeploys staging only if staging is on that branch |
| `shosho-production` | Production deploy | **Never push.** Merge PRs only, with user approval, using a **merge commit** (not squash) |
| `dev/external` | Agent docs, notes, skills, local harness | Yes — deploys nothing. Keep feature code off it |
| `railway/external` | Retired production branch, rollback only | **Never** |

---

## Production Auto-Deploy Warning

**Merging into `shosho-production` = IMMEDIATE PRODUCTION DEPLOYMENT.**

- Before merging any PR into `shosho-production`, confirm with the user: "Is deployment intended right now?"
- Use a merge commit, so production keeps the same commits as the upstream PR.
- Never force-push a branch with an open PR.
- If unsure: **STOP. Do not merge.**

---

## .gitignore Per-Branch Danger

**Each branch has its OWN `.gitignore`.** Files ignored on one branch may be TRACKABLE on another.

`dev/external` has expanded `.gitignore` entries that protect local files. **`main` and any branch off it carry upstream's minimal `.gitignore`, which does NOT ignore `.env`.** This clone is additionally protected by `.git/info/exclude` and a gitleaks pre-commit hook, neither of which survives a fresh clone.

**The disaster scenario:** check out `main` or a feature branch → `git add .` → `docs/deploy/.env` with Cloudflare tokens gets staged → push to the PUBLIC repo → keys burned. This is not hypothetical: a production Cloudflare token sat in this public repo from 2025-12-10 until it was found and rotated on 2026-09-30.

### Rules

1. **Never `git add .` on `main` or a branch off it** — upstream's `.gitignore` won't protect local files
2. **Never `git add -f` or `git add --force`** without explicit user approval
3. **Verify branch before any push:**
   ```bash
   git branch --show-current
   ```
4. **Verify staged files before commit:**
   ```bash
   git diff --cached --name-only
   # Should NOT see: .env, *.local.*, docker-compose.override.*
   ```

---

## Testing Upstream Code

**DO NOT test upstream in the same repository.** Docker bind mounts use the SAME physical data directory regardless of branch. Database contamination will occur.

**The ONLY working approach: separate clone.**

```bash
cd /Users/bchq/projects/zap-stream
git clone https://github.com/v0l/zap-stream-core zap-stream-core-upstream-test
cd zap-stream-core-upstream-test/docs/deploy
docker-compose up --build -d
# Test, then clean up:
docker-compose down -v
```

---

## Pre-Commit Checklist

Before ANY commit, complete ALL steps in order:

### 1. Cargo tests (ALL must pass, no new warnings)

```bash
cargo test -p zap-stream-external
cargo test -p zap-stream-db
```

### 2. Verify no secrets in staged files

```bash
git diff --cached --name-only
# Must NOT include: .env, any file with tokens/nsec/passwords
grep -rn "nsec1\|cfut_\|password" $(git diff --cached --name-only) 2>/dev/null
# Should return nothing (or only comments/placeholders)
```

### 3. Docker integration test (when changes affect runtime)

```bash
cd docs/deploy
docker compose -f docker-compose.override.yml up --build -d
docker compose -f docker-compose.override.yml logs -f zap-stream-external
# Verify: relay connected, webhook registered, listening, no errors
```

### 4. Only then commit

---

## Local Docker Configuration

Local dev testing uses a **self-contained** compose file:

**Tracked in git:**
- `docker-compose.override.yml` — self-contained local dev compose (db + service)
- `config.local.external.yaml` — local config with test relay, secrets commented out

**NOT tracked (gitignored, contain secrets):**
- `.env` — secrets (CF token, nsec, tunnel URL, DB password)

**Usage:**
```bash
cd docs/deploy
docker compose -f docker-compose.override.yml up --build -d
```

Do NOT merge with other compose files. The override is self-contained.

---

## Cloudflare E2E Testing

The Cloudflare integration requires webhooks, which require a cloudflared tunnel for local testing.

A working end-to-end test must demonstrate the COMPLETE lifecycle:
1. User gets their stream key via API (NIP-98 auth)
2. FFmpeg streams to Cloudflare RTMP endpoint
3. Cloudflare sends `live_input.connected` webhook
4. Webhook triggers stream start workflow (DB updates + Nostr publishing)
5. HLS playback works
6. Stream ends
7. Cloudflare sends `live_input.disconnected` webhook
8. Webhook triggers cleanup
9. HLS playback stops

See `crates/zap-stream-external/tests/TESTING_README.md` for the full testing playbook.

---

## GitHub Issues

Issues are tracked in the **shosho-monorepo** GitHub repo, not this repo. If you cannot find issues, ask the user — do NOT run `gh repo set-default`.

---

## Historical Anti-Patterns

These mistakes have happened. Do not repeat them.

| Anti-pattern | What went wrong |
|-------------|-----------------|
| Committing without Docker integration test | Only ran `cargo test`, missed runtime failures |
| Running `docker-compose up --build` without `-d` | Output overflowed AI context window |
| Using `git add -f` to bypass .gitignore | Exposed internal files to GitHub |
| Docker project names (`-p`) for isolation | Bind mounts ignore project names — same data directory |
| Testing upstream on same branch | Database contamination via shared bind mounts |
| Creating branches without asking user | Wrong branch = merge conflicts and wasted work |
| Declaring tests pass without checking END logs | Stream start worked but end was broken |
| Using upstream default config locally | Hardcoded values don't work on other machines |
| Merging compose files for local dev | Port conflicts, phantom volume mounts, confusing behavior |
| Running cloudflared without nohup | Tunnel dies when the calling process exits |
