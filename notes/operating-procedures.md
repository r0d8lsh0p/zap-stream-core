# Operating Procedures

Runbook for AI agents and operators to administer and health-check the zap-stream-external deployment and its Cloudflare Stream integration.

All procedures are read-only unless explicitly stated. Do not modify production state without user approval.

**Common pattern**: Many procedures use `railway run` to inject production env vars into a local command. This avoids exposing credentials — the connection string stays in the env var, never in your command or output.

**Temp directory pattern**: Always `railway link` in a temp directory to avoid associating this repo with a Railway project:
```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && <your command here> \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

---

## 1. Check Railway production logs

### When to use

- After a deploy to verify the service started cleanly
- When a user reports streaming issues (disconnections, failed go-live)
- When investigating the "Database sync issue" log pattern (see issue #817)
- Routine health check

### Prerequisites

- Railway CLI installed: `railway --version`
- Authenticated: `railway whoami` (should show `rodbishop`)
- If not authenticated: ask the user to run `! railway login`

### Procedure

**Step 1: Fetch recent logs**

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway logs --lines 50 \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

For more history, increase `--lines` (e.g. `--lines 200`).

To stream live logs:

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway logs --follow \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

**Step 2: Check for healthy startup**

After a deploy, the logs should show all of these in order:

```
[INFO] Starting zap-stream-cf
[INFO] Using LNURL payment backend: ...
[INFO] Connected to 'wss://...'          (×5 relays)
[INFO] Listening to invoices from ...
[INFO] Webhook notification url already registered: https://api.shosho.live/api/v1/webhook/cloudflare
[INFO] Listening on: [::]:8080
[INFO] Checking 0 live streams..
```

If any of these are missing, the service did not start correctly.

**Step 3: Check for errors**

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway logs --lines 200 2>&1 | grep -iE "error|fatal|panic|WARN" \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

### What to look for

| Log pattern | Meaning | Severity |
|------------|---------|----------|
| `Checking N live streams..` | Normal polling cycle | Info |
| `Published stream event` | Stream started or updated successfully | Info |
| `Stream ended` | Stream ended normally | Info |
| `Webhook signature header missing` | Cloudflare webhook signature not configured — functional but not verified | Low |
| `Database sync issue, live stream is supposed to be live but cloudflare shows the status` | Poll found stream live in DB but disconnected in Cloudflare — may indicate the wrong input ID is being checked (see issue #817) | High |
| `Failed to fetch live input for user` | Cloudflare API call failed for a user's input | High |
| `Relay receiver exited with error` / `Disconnected from` | Nostr relay connection dropped (auto-reconnects) | Low |

### Environment variables

To inspect current environment variables (contains secrets — do not log or share):

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway variables \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

Expected variables (names only, not values):
- `APP__DATABASE` — MariaDB connection string
- `APP__NSEC` — Nostr signing key
- `APP__CLOUDFLARE__TOKEN` — Cloudflare API token
- `APP__CLOUDFLARE__ACCOUNT_ID` — Cloudflare account
- `APP__PUBLIC_URL` — Public URL (`https://api.shosho.live`)
- `APP__ADMIN_PUBKEY` — Admin Nostr pubkey
- `LOG_FORMAT` — Set to `json` for structured logging

If you see any `APP__OVERSEER__*` variables, they are cruft from the old binary and should be removed.

### CLI notes

- `--since` is NOT supported on the installed Railway CLI version — use `--lines` instead
- `--filter` may not be available — pipe through `grep` instead
- Always use a temp directory for `railway link` to avoid associating repo state
- The `railway link` command will print selection prompts even in non-interactive mode — this is normal, the flags still work

---

## 2. Check for zombie live streams

### When to use

- After a user reports a stream that ended but still shows as live
- After the "Database sync issue" log pattern is observed
- After a server restart or crash during an active stream
- Routine health check

### Background

The `user_stream` table tracks stream state. `state = 2` means live. Normally, when a stream ends (via Cloudflare `live_input.disconnected` webhook or the polling check), the server sets `state = 3` (ended). If the server crashes, loses connectivity, or hits a bug during this transition, the record can remain stuck as `state = 2` — a zombie.

The server's `check_streams` poll (every 30s) queries `SELECT * FROM user_stream WHERE state = 2` and checks each against Cloudflare. Zombies cause unnecessary Cloudflare API calls and may trigger false "Database sync issue" warnings.

### Procedure

**Step 1: Check for zombie streams**

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && npm init -y >/dev/null 2>&1 \
  && npm install mysql2 --silent 2>/dev/null \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway run -- node -e "
const mysql = require('mysql2/promise');
(async () => {
  const conn = await mysql.createConnection(process.env.APP__DATABASE);
  const [rows] = await conn.execute(
    'SELECT id, user_id, state, starts, ends, title, stream_key_id FROM user_stream WHERE state = 2'
  );
  console.log('Zombie check - streams with state=2 (live) in DB:', rows.length);
  if (rows.length > 0) {
    rows.forEach(r => console.log(
      '  ID:', r.id,
      '| user:', r.user_id,
      '| started:', r.starts,
      '| title:', r.title || '(none)',
      '| key_id:', r.stream_key_id || 'default'
    ));
  } else {
    console.log('No zombie streams found. All clear.');
  }
  await conn.end();
})();
" 2>&1 \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

**Step 2: Interpret results**

- **0 live streams**: All clear. No action needed.
- **1+ live streams**: Cross-reference with the server logs. If the server is logging `Checking N live streams..` with the same count, the server is already aware and checking them against Cloudflare. If Cloudflare shows them as disconnected, the server should end them on the next poll cycle.
- **Persistent zombies** (still live after several poll cycles): The server's poll may be checking the wrong Cloudflare input (see issue #817). These need manual cleanup.

**Step 3: Fix zombie streams (requires user approval)**

If the user confirms a stream is a zombie and should be ended:

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && npm init -y >/dev/null 2>&1 \
  && npm install mysql2 --silent 2>/dev/null \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway run -- node -e "
const mysql = require('mysql2/promise');
const STREAM_ID = '<STREAM_ID_HERE>';
(async () => {
  const conn = await mysql.createConnection(process.env.APP__DATABASE);
  const [result] = await conn.execute(
    'UPDATE user_stream SET state = 3, ends = NOW() WHERE id = ? AND state = 2',
    [STREAM_ID]
  );
  console.log('Updated', result.affectedRows, 'rows');
  await conn.end();
})();
" 2>&1 \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

**WARNING**: This is a write operation. Only run with explicit user approval. Replace `<STREAM_ID_HERE>` with the actual stream ID from Step 1.

### Notes

- The `mysql2` npm package is wire-compatible with MariaDB — this is the correct client
- `railway run` injects `APP__DATABASE` as an env var — credentials never appear in commands or output
- The npm install happens in a temp directory and is cleaned up automatically
- The server's `check_streams` poll runs every 30 seconds — if you fix a zombie, the next poll should see 0 live streams

---

## 3. Cross-reference Nostr live events with database

### When to use

- After fixing zombie streams (procedure 2) to verify Nostr state matches
- When a stream appears live on the website but the user says it ended
- When a stream ended but still shows as live on Nostr clients
- Routine health check alongside procedure 2

### Background

The zap-stream-external server publishes NIP-53 kind 30311 events to Nostr relays using the server signing key (`npub1sh0cy25xtx0lh6q58kc7rcdl95tzlfs0c6zuv4g4jclx0n7hfx0sghnh3u`, hex `85df822a86599ffbe8143db1e1e1bf2d162fa60fc685c65515963e67cfd7499f`).

**Important**: Two separate services publish kind 30311 events with this same pubkey:
1. **zap-stream-external** (this repo) — Cloudflare-backed streams. Events have `["service", "https://api.shosho.live/api/v1"]`
2. **Show chat monitor** (shosho-monorepo backend) — manages show lifecycle for external RTMP sources. Events do NOT have the `service` tag.

This procedure only concerns events with `["service", "https://api.shosho.live/api/v1"]`.

### Prerequisites

- `nak` CLI installed: `nak --version` (should be 0.18+)
- `jq` installed: `jq --version`
- Railway CLI authenticated (for DB cross-reference)

### Procedure

**Step 1: Query Nostr for live events from our server**

```bash
nak req --kind 30311 \
  --author 85df822a86599ffbe8143db1e1e1bf2d162fa60fc685c65515963e67cfd7499f \
  wss://relay.damus.io wss://nos.lol wss://relay.primal.net 2>/dev/null \
| jq -r '
  select(
    (.tags | map(select(.[0] == "status" and .[1] == "live")) | length > 0) and
    (.tags | map(select(.[0] == "service" and .[1] == "https://api.shosho.live/api/v1")) | length > 0)
  ) | {
    id: .id[0:8],
    d: (.tags | map(select(.[0] == "d")) | .[0][1]),
    title: (.tags | map(select(.[0] == "title")) | .[0][1] // "(none)"),
    host: (.tags | map(select(.[0] == "p")) | .[0][1][0:16]),
    starts: (.tags | map(select(.[0] == "starts")) | .[0][1])
  }'
```

This returns only `status=live` events with our `service` tag. Each result shows the stream `d` tag (which is the `user_stream.id` in the database), the title, host pubkey prefix, and start time.

**Step 2: Query database for live streams**

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && npm init -y >/dev/null 2>&1 \
  && npm install mysql2 --silent 2>/dev/null \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway run -- node -e "
const mysql = require('mysql2/promise');
(async () => {
  const conn = await mysql.createConnection(process.env.APP__DATABASE);
  const [rows] = await conn.execute(
    'SELECT id, user_id, state, starts, title FROM user_stream WHERE state = 2'
  );
  console.log('DB live streams:', rows.length);
  rows.forEach(r => console.log('  ID:', r.id, '| user:', r.user_id, '| started:', r.starts, '| title:', r.title || '(none)'));
  await conn.end();
})();
" 2>&1 \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

**Step 3: Cross-reference**

Compare the `d` tag values from Step 1 with the `id` values from Step 2.

| Nostr has live event | DB has state=2 | Meaning |
|---------------------|----------------|---------|
| Yes | Yes | Healthy — stream is live in both |
| Yes | No | **Nostr zombie** — event says live but DB says ended. The server should have published an `ended` event but didn't. May need manual Nostr event correction. |
| No | Yes | **DB zombie** — DB says live but no live event on Nostr. The server's poll should detect this and end it, or it may be a stream that just started and the event hasn't propagated yet. |
| No | No | Healthy — no active streams |

### Resolving mismatches

- **DB zombies**: Use procedure 2, step 3 to set `state = 3` (requires user approval)
- **Nostr zombies**: The server needs to publish a new 30311 event with `status=ended` for that `d` tag. This typically requires restarting the server or triggering a poll cycle while the stream is still in the DB as live. If the DB record is already ended, manual intervention via nostr tooling may be needed.

### Notes

- The `nak req` command queries multiple relays. Different relays may have different event states due to propagation delays — check at least 3 relays.
- The `d` tag in the Nostr event corresponds to `user_stream.id` in the database (UUID format for Cloudflare-backed streams).
- Events from the show chat monitor (without the `service` tag) will also appear in raw queries — always filter by `service` tag.

---

## 4. Verify live stream poller is running

The server logs `"Checking N live streams.."` every 30 seconds. If these messages stop, the poller has crashed and zombie streams will accumulate. The rest of the service may appear healthy.

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway logs --lines 200 2>&1 | grep "Checking.*live streams" \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

- **Regular messages at ~30s intervals**: Healthy.
- **Messages stopped or absent**: Poller is dead. Redeploy required.

---

## 5. Verify Cloudflare webhook configuration

Cloudflare uses two separate webhook systems. Both must be correctly configured. See `docs/CLOUDFLARE_BACKEND.md` for full details.

### 5a. Stream webhook (video asset events)

The server auto-registers this on startup. One URL per account — dev/test instances can overwrite it.

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway run -- bash -c \
     'curl -s "https://api.cloudflare.com/client/v4/accounts/${APP__CLOUDFLARE__ACCOUNT_ID}/stream/webhook" \
       -H "Authorization: Bearer ${APP__CLOUDFLARE__TOKEN}" | jq .result.notificationUrl' \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

- **Expected**: `"https://api.shosho.live/api/v1/webhook/cloudflare"`
- **Any other URL**: A dev/test instance has overwritten it. Redeploying production will re-register the correct URL.

### 5b. Notification policy (live input connected/disconnected events)

This is a one-time setup per CF account (see r0d8lsh0p/shosho-monorepo#824 for auto-setup). Without it, `live_input.connected` and `live_input.disconnected` events are never delivered.

```bash
tmpdir=$(mktemp -d) && cd "$tmpdir" \
  && railway link --project 6a3ef637-b5ac-4b7a-8c59-eafa71d9ff98 \
     --environment Production --service "ZS Core with CF Stream" \
  && railway run -- bash -c \
     'echo "=== Webhook destinations ===" && \
      curl -s "https://api.cloudflare.com/client/v4/accounts/${APP__CLOUDFLARE__ACCOUNT_ID}/alerting/v3/destinations/webhooks" \
       -H "Authorization: Bearer ${APP__CLOUDFLARE__TOKEN}" | jq ".result[] | {id, name, url}" && \
      echo "=== Notification policies ===" && \
      curl -s "https://api.cloudflare.com/client/v4/accounts/${APP__CLOUDFLARE__ACCOUNT_ID}/alerting/v3/policies" \
       -H "Authorization: Bearer ${APP__CLOUDFLARE__TOKEN}" | jq ".result[] | {id, name, alert_type, enabled}"' \
  && cd - >/dev/null && rm -rf "$tmpdir"
```

- **Expected**: At least one webhook destination pointing to the correct URL, and one policy with `alert_type: stream_live_notifications` and `enabled: true`.
- **Missing or wrong**: Follow the setup instructions in `docs/CLOUDFLARE_BACKEND.md` step 4.
