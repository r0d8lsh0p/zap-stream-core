# Railway Deployment Guide

## Overview

Adding zap-stream-core as a new service in your existing Shosho Railway project.

## Setup Steps

### 1. Add MySQL to Railway Project

In Railway dashboard:
1. Click "+ New" → "Database" → "Add MySQL"
2. Railway automatically creates `MYSQL_URL` environment variable

**Why manual?** Railway plugins (MySQL, PostgreSQL, etc.) can't be configured in railway.toml - they must be added through the dashboard.

### 2. Connect GitHub Repo

Railway will auto-detect the `railway.toml` file and:
- Build using `crates/zap-stream/Dockerfile`
- Auto-deploy on every push to the connected branch

### 3. Set Environment Variables (Secrets Only)

In zap-stream-core service settings → Variables:

```
APP_ADMIN_PUBKEY=<your_admin_pubkey_hex>
APP_OVERSEER__NSEC=<your_nostr_secret_key>
APP_OVERSEER__CLOUDFLARE__API_TOKEN=<cloudflare_api_token>
APP_OVERSEER__CLOUDFLARE__ACCOUNT_ID=<cloudflare_account_id>
APP_OVERSEER__DATABASE=${MYSQL_URL}
```

**Note**: Non-sensitive values like `public_url` are already in `config.railway.yaml` - no need to set them as env vars.

### 4. Configure Cloudflare Webhooks

In Cloudflare Stream dashboard, set webhook URL:
```
https://api.shosho.live/webhooks/cloudflare
```

(Use Railway's provided domain until you configure custom domain)

## Deployment

**Automatic**: Railway auto-deploys on every push to your connected branch (main or railway-deployment).

## Testing

Test the public time endpoint:
```bash
curl https://<railway-domain>/api/v1/time
```

Expected: `{"time": 1734234567890}`

## API Endpoints

See `crates/zap-stream/src/api.rs` for complete endpoint list.

**Public:**
- `GET /api/v1/time`

**Authenticated (NIP-98):**
- `GET /api/v1/account`
- `PATCH /api/v1/account`
- `GET /api/v1/topup`
- `GET /api/v1/history`
- And more...

**Webhooks:**
- `POST /webhooks/cloudflare`

## Monitoring

Check Railway dashboard for:
- Real-time logs
- CPU/memory metrics
- Deployment status
