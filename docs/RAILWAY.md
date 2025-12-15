# Railway Deployment Guide

## Overview

Adding zap-stream-core as a new service in your existing Shosho Railway project.

## Setup Steps

### 1. Add MariaDB to Railway Project

In Railway dashboard:
1. Click "+ New" → "Database" → "Add MariaDB"
2. Railway automatically creates `MARIADB_URL` environment variable

**⚠️ CRITICAL: Use MariaDB, NOT MySQL**

The project uses MariaDB in docker-compose, and the SQL migrations are written for MariaDB compatibility. Using MySQL is impossible, AIs that suggest this are wrong.

Various MariaDB templates exist: Try https://railway.com/deploy/Onvy0F

**Why manual?** Railway plugins (MariaDB, MySQL, PostgreSQL, etc.) can't be configured in railway.toml - they must be added through the dashboard.

### 2. Connect GitHub Repo

Railway will auto-detect the `railway.toml` file and:
- Build using `crates/zap-stream/Dockerfile`
- Auto-deploy on every push to the connected branch

### 3. Set Environment Variables

In zap-stream-core service settings → Variables:

**Required - Secrets:**
```
APP_ADMIN_PUBKEY=<your_admin_pubkey_hex>
APP_OVERSEER__NSEC=<your_nostr_secret_key>
APP_OVERSEER__CLOUDFLARE__API_TOKEN=<cloudflare_api_token>
APP_OVERSEER__CLOUDFLARE__ACCOUNT_ID=<cloudflare_account_id>
APP_OVERSEER__DATABASE=${MARIADB_URL}
```

Using "${MARIADB_URL}" does not work directly. Instead, deploy a MariaDB service to Railway, then select it > Variables > MARIADB_PUBLIC_URL and copy. The connection URL will look similar to "mariadb://railway:tokenabcd@subdomain.rlwy.net:27750/railway". Once copied, visit your Zap Stream Service > variables > APP__OVERSEER__DATABASE and paste the result into that variable field.

**Required - Domain Configuration:**
```
APP__PUBLIC_URL=https://<your-railway-domain>.up.railway.app
```

For staging/testing, use Railway's auto-generated domain (e.g., `https://truthful-tenderness-staging.up.railway.app`). For production, use your custom domain (e.g., `https://api.shosho.live`).

**Why APP__PUBLIC_URL is required:**
- Cloudflare validates webhook URLs by attempting DNS lookup and connection before registration
- Railway's auto-generated domains are immediately resolvable
- Custom domains require DNS configuration to be completed first

---

## ⚠️ CRITICAL: Config File vs Environment Variables

**THE RULE:** If a value exists in `config.railway.yaml`, Railway environment variables for that key are **COMPLETELY IGNORED**.

The Rust `config` crate loads files first, then environment variables. If a key has a value from the YAML file, the environment variable is never checked.

**This is why `config.railway.yaml` comments out all Railway-managed values:**
- ✅ `# public_url:` - Commented out → `APP__PUBLIC_URL` env var loads
- ✅ `# Set via Railway env var: APP_ADMIN_PUBKEY` - Commented out → env var loads
- ✅ Cloudflare settings commented out → env vars load
- ❌ If uncommented → env vars are IGNORED, YAML value is used

**Never uncomment Railway-managed values in `config.railway.yaml`** or you'll be debugging for hours why your environment variables aren't working.

---

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
