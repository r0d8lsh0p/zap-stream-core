# CRITICAL: Git & Safety Strategy for zap-stream-core

**READ THIS BEFORE ANY GIT OPERATIONS OR COMMITS**

This document consolidates hard-won lessons from real incidents. Every rule exists because something went wrong.

---

## Branch Structure

```
[Remote: upstream - v0l/zap-stream-core]
  |
  v
upstream/main (read-only, fetch only)
  |
  v
integration/external    <-- clean commits for upstream submission
  |                         NO local config, NO agent files
  v  (cherry-pick)
railway/external        <-- production deployment branch
                            Railway config + AGENTS.md on top
                            PUSHES TO ORIGIN AUTO-DEPLOY PRODUCTION
```

### Branch Rules

| Branch | Purpose | Push to origin? |
|--------|---------|-----------------|
| `integration/external` | Upstream-submittable code | Yes — but only squash-merged, reviewed commits |
| `railway/external` | Production deploy | **DANGEROUS** — auto-deploys on push. User approval required |
| Feature worktrees | WIP from `integration/external` | Yes — safe to push feature branches |
| `upstream-main` / `test-upstream-*` | Local upstream testing | **NEVER PUSH** — lacks our .gitignore |
| `safe-working-baseline` | Safety net | **NEVER CHANGE** |

---

## Railway Auto-Deploy Warning

**Pushing to `railway/external` on origin = IMMEDIATE PRODUCTION DEPLOYMENT.**

- No staging period, no manual approval, no delay
- Before ANY push, confirm with user: "Is this branch connected to Railway auto-deployment?"
- If yes: Have all tests passed? Is deployment intended right now?
- If unsure: **STOP. Do not push.**

---

## .gitignore Per-Branch Danger

**Each branch has its OWN `.gitignore`.** Files ignored on one branch may be TRACKABLE on another.

```
Our branches (.gitignore):       Upstream branch (.gitignore):
----------------------------     ----------------------------
docs/deploy/data/                # (not listed)
docs/deploy/compose-config.local.yaml   # (not listed)
docs/deploy/docker-compose.override.yaml  # (not listed)
scripts/.env                     scripts/.env
```

**The disaster scenario:** Check out an upstream-based branch → `git add .` → local config with production Nostr keys gets staged → push → keys permanently burned.

### Rules

1. **Verify .gitignore before any git add/push:**
   ```bash
   git branch --show-current
   cat .gitignore | grep -E "data|local|override|scripts"
   ```

2. **Never `git add -f` or `git add --force`** without explicit user approval. If git says "paths are ignored", that's a security protection — STOP and ask the user.

3. **Never push upstream-based branches** — keep them local only.

4. **Verify staged files before commit:**
   ```bash
   git diff --cached --name-only
   # Should NOT see: scripts/.env, *.local.yaml, data/, docker-compose.override.yaml
   ```

---

## Testing Upstream Code

**DO NOT test upstream in the same repository.** Docker bind mounts (`./data/db:/var/lib/mysql/`) use the SAME physical data directory regardless of branch. Database contamination will occur.

**The ONLY working approach: separate clone.**

```bash
cd /Users/visitor/projects/zap-stream
git clone https://github.com/v0l/zap-stream-core zap-stream-core-upstream-test
cp zap-stream-core/docs/deploy/compose-config.local.yaml \
   zap-stream-core-upstream-test/docs/deploy/compose-config.yaml
cd zap-stream-core-upstream-test/docs/deploy
docker-compose up --build -d
# Test, then clean up:
docker-compose down -v
```

**Approaches that DO NOT work:**
- `docker-compose -p upstream` — project names only affect named volumes, not bind mounts
- `git checkout upstream-main` — same `data/` directory, database contamination
- Branch switching — `docker-compose.override.yaml` from feature branch persists

---

## Pre-Commit Checklist

Before ANY commit, complete ALL steps in order:

### 1. Cargo tests (ALL must pass, no new warnings)

```bash
cd /Users/visitor/projects/zap-stream/zap-stream-core
cargo test
```

To check for new warnings, compare with baseline:
```bash
cargo test 2>&1 | grep "generated.*warning"
```
Compare per-package counts with the previous commit. If any count increased, fix before committing.

### 2. Docker integration test

```bash
cd docs/deploy
docker-compose up --build -d   # -d flag is MANDATORY to avoid context overflow
```

Wait for user to confirm containers are running before proceeding.

### 3. Stream start test (ffmpeg)

```bash
ffmpeg -re -f lavfi -i testsrc=size=1280x720:rate=30 \
  -f lavfi -i sine=frequency=1000:sample_rate=44100 \
  -c:v libx264 -preset veryfast -tune zerolatency \
  -c:a aac -ar 44100 \
  -f flv rtmp://localhost:1935/Basic/{STREAM_KEY} \
  </dev/null >/dev/null 2>&1 &
sleep 10
docker logs --tail 50 zap-stream-core-core-1
```

**Required logs:** "Published stream request", "Pipeline run starting", "Published stream event", "Created fMP4 initialization segment"

### 4. Stream end test (DO NOT SKIP)

```bash
pkill -9 -f "ffmpeg.*testsrc"
sleep 5
docker logs --tail 50 zap-stream-core-core-1
```

**Required logs:** "read_data EOF", "Demuxer get_packet failed", "Stream ended", "PipelineRunner cleaned up resources"

### 5. Only then commit

---

## Local Docker Configuration

Uses the Docker Compose override pattern for local dev:

**Tracked in git (receives upstream updates):**
- `docker-compose.yaml` — base config (pulls from Docker Hub)
- `compose-config.yaml` — config template

**NOT tracked (local customizations, protected by .gitignore):**
- `docker-compose.override.yaml` — local overrides (build from source, local config path)
- `compose-config.local.yaml` — actual configuration with credentials
- `data/` — Docker volumes

Docker Compose automatically merges `docker-compose.yaml` + `docker-compose.override.yaml`. No flags needed.

---

## Cloudflare E2E Testing

The Cloudflare integration requires webhooks, which require a tunnel for local testing.

**The tunnel is the missing piece** — without it, Cloudflare cannot reach the local server, and no webhooks arrive.

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

See `notes/cloudflare-integration-test-plan.md` for the step-by-step test procedure.

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
| Counting warnings with `grep "^warning:" \| wc -l` | Wrong count — use `grep "generated.*warning"` instead |
| Using upstream default config locally | Hardcoded values don't work on other machines |
| Continuing after repeated failures without acknowledging | Wastes time — after 2-3 major failures, stop and reassess |
