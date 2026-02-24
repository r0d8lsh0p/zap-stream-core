# Railway Deployment Guide (zap-stream-external)

## Overview

Deploying the `zap-stream-external` Cloudflare Live Stream backend as a service in the Shosho Railway project.

## Setup Steps

### 1. Add MariaDB to Railway Project

In Railway dashboard:
1. Visit a MariaDB template e.g. https://railway.com/deploy/Onvy0F
2. Click to Deploy that to your project
3. Railway automatically creates the required variables, including `MARIADB_PUBLIC_URL`

**Why manual?** Railway plugins can't be configured in railway.toml - they must be added through the dashboard.

### 2. Connect GitHub Repo

In Railway dashboard:
1. Click "Create + " and connect to your Github repo
2. Railway will detect the Dockerfile and `railway.toml` but they still need to be selected and configured manually

- Build using `crates/zap-stream-external/Dockerfile`
- Configuration as code using `railway.toml`
- Auto-deploy on every push to the connected branch
- "Wait for CI" recommended, to run any automated test suite prior to deployment

### 3. Set Environment Variables

In Railway dashboard, Service settings > Variables:

**Required - Secrets:**
```
APP__DATABASE=<your_MariaDB_connection_URL>
APP__NSEC=<your_nostr_secret_key>
APP__CLOUDFLARE__TOKEN=<cloudflare_api_token>
APP__CLOUDFLARE__ACCOUNT_ID=<cloudflare_account_id>
```

Note: using `${MARIADB_URL}` as a variable reference does not work. Instead, from the MariaDB service you deployed in Step 1, select MariaDB Service > Variables > `MARIADB_PUBLIC_URL` and copy. The connection URL will look similar to `mariadb://railway:tokenabcd@subdomain.rlwy.net:27750/railway`. Paste it into `APP__DATABASE`.

**Required - Domain Configuration:**
```
APP__PUBLIC_URL=https://<your-railway-domain>.up.railway.app
```

- For staging/testing, use Railway's auto-generated domain (e.g., `https://zs-core-with-cf-stream-staging.up.railway.app`).
- For production, use your custom domain (e.g., `https://api.shosho.live`).

**Why APP__PUBLIC_URL is required:**
- Cloudflare validates webhook URLs by attempting DNS lookup and connection before registration
- Railway's auto-generated domains are immediately resolvable
- Custom domains require DNS configuration to be completed first

### 4. Cloudflare Configuration

The deployment uses Cloudflare for stream ingest and requires webhook notifications.

See `CLOUDFLARE_BACKEND.md` for full Cloudflare setup including:
- API token creation
- Webhook destination configuration
- Custom ingest domain setup (optional)

---

## Config File vs Environment Variables

**THE RULE:** If a value exists in `config.railway.external.yaml`, Railway environment variables for that key are **COMPLETELY IGNORED**.

The Rust `config` crate loads files first, then environment variables. If a key has a value from the YAML file, the environment variable is never checked.

**This is why `config.railway.external.yaml` comments out all Railway-managed values:**
- `# database:` → `APP__DATABASE` env var loads
- `# public_url:` → `APP__PUBLIC_URL` env var loads
- `# nsec:` → `APP__NSEC` env var loads
- `# cloudflare:` → `APP__CLOUDFLARE__TOKEN` and `APP__CLOUDFLARE__ACCOUNT_ID` env vars load

**Never uncomment Railway-managed values in `config.railway.external.yaml`** or you'll be debugging for hours why your environment variables aren't working.

### Environment Variable Format

Environment variables use `APP__` prefix with double underscore (`__`) as separator for nested keys:
- `APP__DATABASE` → `database`
- `APP__PUBLIC_URL` → `public_url`
- `APP__NSEC` → `nsec`
- `APP__CLOUDFLARE__TOKEN` → `cloudflare.token`
- `APP__CLOUDFLARE__ACCOUNT_ID` → `cloudflare.account_id`

---

## Deployment

**Every Push = Immediate Deployment**

Railway is configured to **automatically deploy** on every push to the connected branch.

**This means:**
- `git push` → **IMMEDIATE DEPLOYMENT**
- No manual approval required
- Changes go live within minutes

**DO NOT push unless:**
- You intend to deploy immediately
- Changes have been tested locally
- You are ready for deployment

**For development work:**
- Work on a different branch
- Test thoroughly before merging/pushing
- Only push when you're ready to deploy

## Testing

Test the public time endpoint:
```bash
curl https://<railway-domain>/api/v1/time
```

Expected: `{"time": 1734234567890}`

## API Endpoints

See `crates/zap-stream-external/src/cloudflare/api.rs` for complete endpoint list.

**Public:**
- `GET /api/v1/time`

**Authenticated (NIP-98):**
- `GET /api/v1/account`
- `PATCH /api/v1/account`
- `GET /api/v1/topup`
- `GET /api/v1/history`

**Webhooks:**
- `POST /webhooks/cloudflare`

## Monitoring

Check Railway dashboard for:
- Real-time logs
- CPU/memory metrics
- Deployment status

## Migration from zap-stream (core)

If migrating from the old `zap-stream` core binary, the environment variables have changed:

| Old (core) | New (external) |
|---|---|
| `APP__OVERSEER__CLOUDFLARE__API-TOKEN` | `APP__CLOUDFLARE__TOKEN` |
| `APP__OVERSEER__CLOUDFLARE__ACCOUNT-ID` | `APP__CLOUDFLARE__ACCOUNT_ID` |
| `APP__OVERSEER__DATABASE` | `APP__DATABASE` |
| `APP__OVERSEER__NSEC` | `APP__NSEC` |
| `APP__PUBLIC_URL` | `APP__PUBLIC_URL` (unchanged) |
| `APP__ADMIN_PUBKEY` | _(removed)_ |
