mod common;

use common::api_client::ApiClient;
use common::config::TestConfig;
use common::db::TestDb;
use common::docker;
use common::ffmpeg::FfmpegStream;
use common::nostr_relay::{self, NostrRelay};
use nostr_sdk::{Event, Keys, Timestamp, ToBech32};
use std::time::Duration;

/// The account key's reconnect window, as seen by the server.
const RECONNECT_WINDOW_SECS: i64 = 120;
const RELAY_WAIT: Duration = Duration::from_secs(60);
const RELAY_POLL: Duration = Duration::from_secs(5);

/// Poll the relay until the kind 30311 at `d_tag` carries the expected status.
async fn await_status(relay: &NostrRelay, since: Timestamp, d_tag: &str, status: &str) -> Event {
    let deadline = tokio::time::Instant::now() + RELAY_WAIT;
    loop {
        let events = relay.query_30311_events(since, Some(d_tag)).await;
        let seen: Vec<String> = events
            .iter()
            .filter_map(|e| nostr_relay::get_tag_value(e, "status"))
            .collect();
        if let Some(found) = events
            .into_iter()
            .find(|e| nostr_relay::get_tag_value(e, "status").as_deref() == Some(status))
        {
            return found;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Timed out waiting for status={status} on d={d_tag}; saw {seen:?}"
        );
        tokio::time::sleep(RELAY_POLL).await;
    }
}

fn tag_ts(event: &Event, name: &str) -> i64 {
    nostr_relay::get_tag_value(event, name)
        .unwrap_or_else(|| panic!("event has no {name} tag"))
        .parse()
        .unwrap_or_else(|_| panic!("{name} is not a timestamp"))
}

struct Account {
    client: ApiClient,
    db: TestDb,
    ext_container: String,
    rtmp_url: String,
    rtmp_key: String,
    input_id: String,
}

impl Account {
    /// Stream on the account key and wait for Cloudflare's connected webhook.
    /// Returns the running encoder and the account-key stream it produced.
    async fn go_live(&self) -> (FfmpegStream, String) {
        let begin = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        let mut ffmpeg = FfmpegStream::start_rtmps(&self.rtmp_url, &self.rtmp_key, 120, 1000).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(ffmpeg.is_running(), "FFmpeg died immediately");
        tokio::time::sleep(Duration::from_secs(20)).await;
        let logs = docker::get_docker_logs_since(&self.ext_container, &begin).await;
        assert!(
            logs.contains(&format!("live_input.connected for input_id: {}", self.input_id)),
            "Missing connected webhook for the account input in logs since {begin}"
        );
        let stream_id = self
            .db
            .get_latest_stream_id(&self.client.pubkey_hex())
            .await
            .expect("no account-key stream after going live");
        (ffmpeg, stream_id)
    }
}

/// Reconnecting on the account key within 120s of the stream ending resumes the same
/// stream — same d tag, original `starts` — and reconnecting after the window starts a
/// fresh one. Custom keys now begin a new session each time they go live; the account
/// key never takes that path, and this pins that it behaves exactly as before.
#[tokio::test]
#[ignore]
async fn e2e_account_key_reconnect_window() {
    let total = 6;
    let config = TestConfig::from_env();
    assert!(
        docker::check_docker_available().await,
        "Docker is not running"
    );
    assert!(
        docker::command_exists("ffmpeg").await,
        "ffmpeg not found on PATH"
    );
    let ext_container = docker::detect_container("zap-stream-external")
        .await
        .or(config.external_container.clone())
        .expect("Cannot find zap-stream-external container");

    let nsec = Keys::generate()
        .secret_key()
        .to_bech32()
        .expect("bech32 nsec");
    let client = ApiClient::new(&nsec, &config.api_base_url()).await;
    let db = TestDb::connect(&config.db_connection_string()).await;
    db.ensure_user_exists(&client.pubkey_hex()).await;
    let account_info = client.get_account().await;
    let rtmps = account_info["endpoints"]
        .as_array()
        .expect("no endpoints")
        .iter()
        .find(|e| e["name"].as_str().unwrap_or("").starts_with("RTMPS-"))
        .expect("No RTMPS endpoint")
        .clone();
    let input_id = db
        .get_external_id(&client.pubkey_hex())
        .await
        .expect("account has no Cloudflare input");
    let account = Account {
        rtmp_url: rtmps["url"].as_str().expect("endpoint url").to_string(),
        rtmp_key: rtmps["key"].as_str().expect("endpoint key").to_string(),
        client,
        db,
        ext_container,
        input_id,
    };
    let relay = NostrRelay::connect(&config.nostr_relay_url).await;
    let since = Timestamp::from(chrono::Utc::now().timestamp() as u64 - 60);

    // ── Step 1: Go live on the account key ─────────────────────────────
    println!("[TEST] Step 1/{total}: Go live on the account key");
    let (mut ffmpeg, stream_id) = account.go_live().await;
    let live = await_status(&relay, since, &stream_id, "live").await;
    let original_starts = tag_ts(&live, "starts");
    assert_eq!(
        account.db.get_stream_starts(&stream_id).await,
        Some(original_starts)
    );
    println!("[PASS] Step 1/{total}: live as {stream_id}");

    // ── Step 2: End it ─────────────────────────────────────────────────
    println!("[TEST] Step 2/{total}: End the stream");
    ffmpeg.stop().await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let ended = await_status(&relay, since, &stream_id, "ended").await;
    let ended_at = tag_ts(&ended, "ends");
    println!("[PASS] Step 2/{total}: ended");

    // ── Step 3: Reconnect inside the window — the same stream resumes ──
    println!("[TEST] Step 3/{total}: Reconnect within {RECONNECT_WINDOW_SECS}s");
    let reconnect_at = chrono::Utc::now().timestamp();
    let (mut ffmpeg, resumed_id) = account.go_live().await;
    let connected_after = chrono::Utc::now().timestamp() - ended_at;
    assert!(
        connected_after < RECONNECT_WINDOW_SECS - 15,
        "reconnected {connected_after}s after the end; too close to the window to test it"
    );
    assert_eq!(
        resumed_id, stream_id,
        "a reconnect inside the window started a new stream instead of resuming"
    );
    let resumed = await_status(&relay, since, &stream_id, "live").await;
    assert_eq!(
        tag_ts(&resumed, "starts"),
        original_starts,
        "resuming changed the stream's start time"
    );
    assert_eq!(
        account.db.get_stream_starts(&stream_id).await,
        Some(original_starts),
        "resuming changed the stored start time"
    );
    println!(
        "[PASS] Step 3/{total}: resumed the same stream {}s after the end, start unchanged",
        reconnect_at - ended_at
    );

    // ── Step 4: End it again ───────────────────────────────────────────
    println!("[TEST] Step 4/{total}: End the resumed stream");
    ffmpeg.stop().await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let ended_again = await_status(&relay, since, &stream_id, "ended").await;
    let ended_again_at = tag_ts(&ended_again, "ends");
    println!("[PASS] Step 4/{total}: ended");

    // ── Step 5: Wait out the window ────────────────────────────────────
    println!("[TEST] Step 5/{total}: Wait out the {RECONNECT_WINDOW_SECS}s window");
    let resume_after = ended_again_at + RECONNECT_WINDOW_SECS + 10;
    let wait = resume_after - chrono::Utc::now().timestamp();
    if wait > 0 {
        tokio::time::sleep(Duration::from_secs(wait as u64)).await;
    }
    println!("[PASS] Step 5/{total}: window elapsed");

    // ── Step 6: Go live after the window — a fresh stream ──────────────
    println!("[TEST] Step 6/{total}: Go live after the window");
    let fresh_at = chrono::Utc::now().timestamp();
    let (mut ffmpeg, fresh_id) = account.go_live().await;
    assert_ne!(
        fresh_id, stream_id,
        "going live after the window resumed the old stream instead of starting a new one"
    );
    let fresh = await_status(&relay, since, &fresh_id, "live").await;
    let fresh_starts = tag_ts(&fresh, "starts");
    assert!(
        (fresh_starts - fresh_at).abs() < 300,
        "the new stream's start ({fresh_starts}) is not when it went live ({fresh_at})"
    );
    ffmpeg.stop().await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    await_status(&relay, since, &fresh_id, "ended").await;
    println!("[PASS] Step 6/{total}: new stream {fresh_id} with its own start");
    relay.disconnect().await;
}
