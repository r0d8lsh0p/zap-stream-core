---
name: railway-logs
description: Read Railway service logs for the zap-stream-external production service. Use when fetching logs, tailing, or filtering the streaming backend.
---

# Railway logs

Use this skill to read Railway logs for the zap-stream-external service.

## Service identifiers

| Field | Value |
|-------|-------|
| Project | Shosho Services |
| Project ID | `6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98` |
| Service name | `ZS Core with CF Stream` |
| Production environment | `Production` |
| Staging environment | `Staging` (if exists) |

## How to read logs

Always use a temporary directory to avoid modifying repo state:

```bash
# Production logs — last 50 lines
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway logs --lines 50 \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

### Common options

```bash
railway logs --lines 50          # Last 50 lines
railway logs --lines 200         # More history
railway logs --follow            # Stream live (no --lines)
railway logs --latest            # Latest deployment (even if failed)
```

Note: `--since` is NOT supported on the installed Railway CLI version. Use `--lines` instead.

### Filtering

The Railway CLI `--filter` flag may not be available. Use `grep` instead:

```bash
# Error-focused
railway logs --lines 200 2>&1 | grep -iE "error|fatal|panic"

# Webhook events
railway logs --lines 200 2>&1 | grep -i "webhook"

# Stream lifecycle
railway logs --lines 200 2>&1 | grep -i "live_input\|stream.*ended\|Database sync"
```

## Reading environment variables

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway variables \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

## Safety notes

- Read-only operations only — no write/deploy/delete
- Always use a temp directory for `railway link` to avoid associating this repo
- The production service auto-deploys from `railway/external` — never push without user approval
- Logs contain structured JSON (LOG_FORMAT=json is set in production)
