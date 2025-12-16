# Railway Deployment Guide

## Overview

Adding zap-stream-core as a new service in your existing Shosho Railway project.

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

- Build using `crates/zap-stream/Dockerfile`
- Congifuration as code using `railway.toml`
- Auto-deploy on every push to the connected branch
- "Wait for CI" recommended, to run any autoamated test suite prior to deployment

### 3. Set Environment Variables

In Railway dashboard
Service settings > Variables:

**Required - Secrets:**
```
APP_ADMIN_PUBKEY=<your_admin_pubkey_hex>
APP_OVERSEER__CLOUDFLARE__API-TOKEN=<cloudflare_api_token>
APP_OVERSEER__CLOUDFLARE__ACCOUNT-ID=<cloudflare_account_id>
APP_OVERSEER__DATABASE=<your_MariaDB_connection_URL>
APP_OVERSEER__NSEC=<your_nostr_secret_key>
```

Note using "${MARIADB_URL}" as a variable does not work. Instead, from the a MariaDB service you deployed in Step 1, select MariaDB Service > variables > MARIADB_PUBLIC_URL and copy. The connection URL will look similar to "mariadb://railway:tokenabcd@subdomain.rlwy.net:27750/railway". Once copied, visit your Zap Stream Service > variables > APP__OVERSEER__DATABASE and paste the result into that variable field.

**Required - Domain Configuration:**
```
APP__PUBLIC_URL=https://<your-railway-domain>.up.railway.app
```

- For staging/testing, use Railway's auto-generated domain (e.g., `https://truthful-tenderness-staging.up.railway.app`). 
- For production, use your custom domain (e.g., `https://api.shosho.live`).

**Why APP__PUBLIC_URL is required:**
- Cloudflare validates webhook URLs by attempting DNS lookup and connection before registration
- Railway's auto-generated domains are immediately resolvable
- Custom domains require DNS configuration to be completed first

### 4. Cloud Flare Configuration

The Shosho deployment to Railway uses Cloud Flare for stream ingest, and accordingly also needs Cloud Flare webhook notifications set up to function correctly. 

If you are using Cloud Flare Live Stream, review the CLOUDFLARE_BACKEND.md document in this folder for more information.

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

## Deployment

**⚠️ CRITICAL: Every Push = Immediate Deployment**

Railway is configured to **automatically deploy** on every push to the connected branch.

**This means:**
- `git push` → **IMMEDIATE DEPLOYMENT**
- No manual approval required
- No delay between push and deployment
- Changes go live within minutes

**DO NOT push to Github unless:**
- You intend to deploy immediately
- Changes have been tested locally
- You are ready for deployment

**For development work:**
- Work on a different branch
- Test thoroughly before merging/pushing
- Only push when you're ready to deploy

**⚠️ CRITICAL: FINAL CHECK**

Do you have explicit recent knowledge that the branch you are on is not linked to an automatic railway deployment? If you have such explicit recent knowledge you may deploy if that is your intent. Otherwise, use ask_question for your user to confirm for you.

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
