---
name: railway-logs
description: Read Railway service logs for the zap-stream-external production service. Use when fetching logs, tailing, or filtering the streaming backend.
---

# Railway logs

Use this skill to read Railway logs for the zap-stream-external production service.

## Quick reference

```bash
# Last 5 minutes of logs
railway logs --since 5m

# Last 100 lines
railway logs --lines 100

# Stream live
railway logs --follow

# Filter for errors
railway logs --since 5m | grep -i error
```

## Prerequisites

- Railway CLI installed and authenticated: `railway login`
- Linked to the correct project/service: `railway link`
- If not linked, the CLI will prompt you to select project and service

## Safety notes

- Read-only operation — no write/deploy/delete
- If running from this repo, `railway link` may prompt to associate — use a temporary directory to avoid modifying repo state:
  ```bash
  tmpdir=$(mktemp -d) && cd "$tmpdir" && railway link && railway logs --since 5m && cd - && rm -rf "$tmpdir"
  ```
- The production service runs the `zap-stream-external` binary on the `railway/external` branch
