mod common;

use common::api_client::ApiClient;
use common::config::TestConfig;
use common::db::TestDb;
use common::docker;
use common::ffmpeg::FfmpegStream;
use common::nostr_relay::{self, NostrRelay};
use nostr_sdk::{Event, Keys, PublicKey, Timestamp, ToBech32};
use std::time::Duration;
use uuid::Uuid;

/// How long to wait for a published event to show up on the relay.
const RELAY_WAIT: Duration = Duration::from_secs(60);
const RELAY_POLL: Duration = Duration::from_secs(5);

/// Poll the relay until a kind 30311 with `d == d_tag` carries the expected status.
/// Returns that event. Panics with the statuses actually seen on timeout.
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

/// Wait for Cloudflare's recording-ready webhook for the episode that just ended. That
/// webhook republishes the show's shared row whatever state it is in — a known, separate
/// issue — so letting it land first keeps it from republishing mid-step. Returns the
/// recording now on the row; if none arrives in time, recordings are presumably off and
/// the webhook will not fire at all.
async fn wait_for_recording(db: &TestDb, stream_id: &str, previous: Option<&str>) -> Option<String> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(180);
    loop {
        let current = db.get_external_video_id(stream_id).await;
        if current.is_some() && current.as_deref() != previous {
            return current;
        }
        if tokio::time::Instant::now() >= deadline {
            println!("[INFO] No recording webhook within 180s; continuing");
            return current;
        }
        tokio::time::sleep(RELAY_POLL).await;
    }
}

/// The address of a replaceable event: (author, d tag). This is what must not move.
fn address(event: &Event) -> (PublicKey, String) {
    (
        event.pubkey,
        nostr_relay::get_tag_value(event, "d").expect("event has no d tag"),
    )
}

struct Harness {
    config: TestConfig,
    client: ApiClient,
    db: TestDb,
    ext_container: String,
    rtmp_url: String,
    run_id: String,
}

async fn boot(require_ffmpeg: bool) -> Harness {
    assert!(
        docker::check_docker_available().await,
        "Docker is not running"
    );
    if require_ffmpeg {
        assert!(
            docker::command_exists("ffmpeg").await,
            "ffmpeg not found on PATH"
        );
    }
    let config = TestConfig::from_env();
    let ext_container = docker::detect_container("zap-stream-external")
        .await
        .or(config.external_container.clone())
        .expect("Cannot find zap-stream-external container");
    docker::detect_container("db-1")
        .await
        .or(config.db_container.clone())
        .expect("Cannot find db container");

    let nsec = Keys::generate()
        .secret_key()
        .to_bech32()
        .expect("bech32 nsec");
    let client = ApiClient::new(&nsec, &config.api_base_url()).await;
    let db = TestDb::connect(&config.db_connection_string()).await;
    db.ensure_user_exists(&client.pubkey_hex()).await;

    let account = client.get_account().await;
    let rtmp_url = account["endpoints"]
        .as_array()
        .expect("no endpoints")
        .iter()
        .find(|e| e["name"].as_str().unwrap_or("").starts_with("RTMPS-"))
        .expect("No RTMPS endpoint")["url"]
        .as_str()
        .expect("endpoint url")
        .to_string();

    let run_id = Uuid::new_v4().to_string()[..8].to_string();
    println!("[INFO] Test run_id: {run_id}");
    Harness {
        config,
        client,
        db,
        ext_container,
        rtmp_url,
        run_id,
    }
}

/// A show announced for the future must be published as `planned`, then move to
/// `live` and `ended` **at the same address** when the user actually streams to it.
#[tokio::test]
#[ignore]
async fn e2e_planned_show_lifecycle() {
    let total = 13;
    let h = boot(true).await;
    let relay = NostrRelay::connect(&h.config.nostr_relay_url).await;
    let since = Timestamp::from(chrono::Utc::now().timestamp() as u64 - 60);

    // ── Step 1: Announce a show two hours from now ─────────────────────
    println!("[TEST] Step 1/{total}: Announce a planned show");
    let starts = chrono::Utc::now() + chrono::Duration::hours(2);
    let ends = starts + chrono::Duration::hours(1);
    let title = format!("Planned Show {}", h.run_id);
    let created = h
        .client
        .create_key_scheduled(
            &title,
            &format!("Announced in advance {}", h.run_id),
            &["planned", &h.run_id],
            Some(&starts.to_rfc3339()),
            Some(&ends.to_rfc3339()),
        )
        .await;
    let key = created["key"].as_str().expect("no key").to_string();
    assert!(!key.is_empty(), "Key is empty");
    // The response must carry the event that was published, not a TODO
    let created_event = created["event"]
        .as_str()
        .expect("POST /keys did not return the published event");
    assert!(
        created_event.contains("planned"),
        "Returned event is not planned: {created_event}"
    );
    println!("[PASS] Step 1/{total}: Key created and event returned");

    // ── Step 2: Read the show back off the API ─────────────────────────
    println!("[TEST] Step 2/{total}: Read back via GET /keys and GET /stream/{{id}}");
    let keys = h.client.list_keys().await;
    let entry = keys
        .as_array()
        .expect("keys is not an array")
        .iter()
        .find(|k| k["key"].as_str() == Some(&key))
        .expect("created key missing from GET /keys")
        .clone();
    let stream_id = entry["stream_id"].as_str().expect("no stream_id").to_string();
    let embedded = &entry["stream"];
    assert_eq!(
        embedded["state"].as_str(),
        Some("planned"),
        "GET /keys did not report the show as planned: {embedded}"
    );
    assert_eq!(
        embedded["title"].as_str(),
        Some(title.as_str()),
        "GET /keys lost the title"
    );
    assert_eq!(
        embedded["starts"].as_i64(),
        Some(starts.timestamp()),
        "GET /keys reported the wrong start time"
    );
    assert_eq!(
        embedded["ends"].as_i64(),
        Some(ends.timestamp()),
        "GET /keys reported the wrong end time"
    );

    let single = h.client.get_stream(&stream_id).await;
    assert_eq!(single["state"].as_str(), Some("planned"));
    assert_eq!(single["title"].as_str(), Some(title.as_str()));
    assert_eq!(single["starts"].as_i64(), Some(starts.timestamp()));
    println!("[PASS] Step 2/{total}: Read-back matches what was created");

    // ── Step 3: The relay has a planned event ──────────────────────────
    println!("[TEST] Step 3/{total}: Validate the planned event on the relay");
    let planned = await_status(&relay, since, &stream_id, "planned").await;
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "d").as_deref(),
        Some(stream_id.as_str()),
        "planned event d-tag mismatch"
    );
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "starts"),
        Some(starts.timestamp().to_string()),
        "planned event advertises the wrong start time"
    );
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "ends"),
        Some(ends.timestamp().to_string()),
        "planned event advertises the wrong end time"
    );
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "title").as_deref(),
        Some(title.as_str()),
        "planned event has the wrong title"
    );
    // Nothing to watch yet, and nobody watching
    assert!(
        !nostr_relay::has_tag(&planned, "streaming"),
        "planned event must not carry a streaming URL"
    );
    assert!(
        !nostr_relay::has_tag(&planned, "current_participants"),
        "planned event must not carry a viewer count"
    );
    let t_tags = nostr_relay::get_all_tag_values(&planned, "t");
    assert!(
        t_tags.contains(&h.run_id),
        "planned event missing run_id t-tag (got {t_tags:?})"
    );
    let planned_addr = address(&planned);
    println!(
        "[PASS] Step 3/{total}: planned event verified at {}:{}",
        planned_addr.0.to_hex(),
        planned_addr.1
    );

    // ── Step 4: Edit the announcement, expect a republish ──────────────
    println!("[TEST] Step 4/{total}: Edit the announced show");
    let new_title = format!("Planned Show {} (updated)", h.run_id);
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": stream_id, "title": new_title }))
        .await;
    assert!(status.is_success(), "PATCH /event returned {status}");

    let deadline = tokio::time::Instant::now() + RELAY_WAIT;
    let edited = loop {
        let ev = await_status(&relay, since, &stream_id, "planned").await;
        if nostr_relay::get_tag_value(&ev, "title").as_deref() == Some(new_title.as_str()) {
            break ev;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "Edited title never reached the relay"
        );
        tokio::time::sleep(RELAY_POLL).await;
    };
    assert_eq!(
        address(&edited),
        planned_addr,
        "the edit moved the show to a different address"
    );
    assert!(
        edited.created_at >= planned.created_at,
        "replacement event is not newer than the original"
    );
    println!("[PASS] Step 4/{total}: Edit republished at the same address");

    // ── Step 5: Go live on the announced key ───────────────────────────
    println!("[TEST] Step 5/{total}: Stream to the planned show's key");
    let external_id = h
        .db
        .get_custom_key_external_id(&stream_id)
        .await
        .expect("no external_id for the key");
    let stream_begin = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let go_live_at = chrono::Utc::now().timestamp();

    let mut ffmpeg = FfmpegStream::start_rtmps(&h.rtmp_url, &key, 120, 1000).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(ffmpeg.is_running(), "FFmpeg died immediately");

    tokio::time::sleep(Duration::from_secs(20)).await;
    let logs = docker::get_docker_logs_since(&h.ext_container, &stream_begin).await;
    let marker = format!("live_input.connected for input_id: {external_id}");
    assert!(
        logs.contains(&marker),
        "Missing webhook '{marker}' in logs since {stream_begin} ({} bytes)",
        logs.len()
    );
    println!("[PASS] Step 5/{total}: Stream started, webhook received");

    // ── Step 6: The SAME event is now live ─────────────────────────────
    println!("[TEST] Step 6/{total}: Validate the live event at the same address");
    let live = await_status(&relay, since, &stream_id, "live").await;
    assert_eq!(
        address(&live),
        planned_addr,
        "going live moved the show to a different address"
    );
    assert!(
        nostr_relay::has_tag(&live, "streaming"),
        "live event is missing a streaming URL"
    );
    // NIP-53: `starts` becomes when the show actually went live, not the announced time
    let live_starts: i64 = nostr_relay::get_tag_value(&live, "starts")
        .expect("live event has no starts tag")
        .parse()
        .expect("starts is not a timestamp");
    assert_ne!(
        live_starts,
        starts.timestamp(),
        "live event still advertises the announced start time"
    );
    assert!(
        (live_starts - go_live_at).abs() < 300,
        "live starts ({live_starts}) is not close to when it went live ({go_live_at})"
    );
    assert_eq!(
        h.db.get_stream_starts(&stream_id).await,
        Some(live_starts),
        "database starts does not match the published starts"
    );
    println!("[PASS] Step 6/{total}: live at the same address, starts = actual go-live");

    // ── Step 7: Stop, expect ended at the same address ─────────────────
    println!("[TEST] Step 7/{total}: Stop streaming");
    ffmpeg.stop().await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let ended = await_status(&relay, since, &stream_id, "ended").await;
    assert_eq!(
        address(&ended),
        planned_addr,
        "ending moved the show to a different address"
    );
    println!("[PASS] Step 7/{total}: ended at the same address");

    // ── Step 8: One address for the whole lifecycle ────────────────────
    println!("[TEST] Step 8/{total}: Confirm a single address across the lifecycle");
    for (label, ev) in [("planned", &planned), ("live", &live), ("ended", &ended)] {
        assert_eq!(
            address(ev),
            planned_addr,
            "{label} event is at a different address"
        );
    }
    println!("[PASS] Step 8/{total}: one naddr for planned, live and ended");

    // ── Step 9: Prepare the next episode while offline ──────────────────
    println!("[TEST] Step 9/{total}: Edit the show's details while it is offline");
    let episode_one = wait_for_recording(&h.db, &stream_id, None).await;
    let next_title = format!("Planned Show {} (episode 2)", h.run_id);
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": stream_id, "title": next_title }))
        .await;
    assert!(
        status.is_success(),
        "an ended custom-key show rejected an edit for its next episode ({status})"
    );
    // Saved for the next go-live, not published over the previous episode's ended event
    tokio::time::sleep(Duration::from_secs(5)).await;
    let on_relay = relay.query_30311_events(since, Some(&stream_id)).await;
    let latest = on_relay.first().expect("the show vanished from the relay");
    assert_eq!(
        nostr_relay::get_tag_value(latest, "status").as_deref(),
        Some("ended"),
        "the edit changed the published status"
    );
    assert_eq!(
        nostr_relay::get_tag_value(latest, "title").as_deref(),
        Some(new_title.as_str()),
        "the edit was published while the show was offline"
    );
    let saved = h.client.get_stream(&stream_id).await;
    assert_eq!(saved["state"].as_str(), Some("ended"));
    assert_eq!(saved["title"].as_str(), Some(next_title.as_str()), "the edit was not saved");
    println!("[PASS] Step 9/{total}: saved, nothing published");

    // ── Step 10: Go live again — the next episode carries the new details ──
    println!("[TEST] Step 10/{total}: Go live for the next episode");
    let begin_two = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let go_live_two = chrono::Utc::now().timestamp();
    let mut ffmpeg = FfmpegStream::start_rtmps(&h.rtmp_url, &key, 120, 1000).await;
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(ffmpeg.is_running(), "FFmpeg died immediately on episode 2");
    tokio::time::sleep(Duration::from_secs(20)).await;
    let logs = docker::get_docker_logs_since(&h.ext_container, &begin_two).await;
    assert!(
        logs.contains(&format!("live_input.connected for input_id: {external_id}")),
        "Missing connected webhook for episode 2 in logs since {begin_two}"
    );
    let live_two = await_status(&relay, since, &stream_id, "live").await;
    assert_eq!(
        address(&live_two),
        planned_addr,
        "episode 2 went live at a different address"
    );
    assert_eq!(
        nostr_relay::get_tag_value(&live_two, "title").as_deref(),
        Some(next_title.as_str()),
        "episode 2 went live with the previous episode's details"
    );
    // Each episode advertises its own start, not the previous episode's
    let starts_two: i64 = nostr_relay::get_tag_value(&live_two, "starts")
        .expect("episode 2 has no starts tag")
        .parse()
        .expect("starts is not a timestamp");
    assert!(
        (starts_two - go_live_two).abs() < 300,
        "episode 2 starts ({starts_two}) is not close to when it went live ({go_live_two})"
    );
    assert!(starts_two > live_starts, "episode 2 still advertises episode 1's start");
    println!("[PASS] Step 10/{total}: live with the new details and its own start, same d tag");

    // ── Step 11: End episode 2 ─────────────────────────────────────────
    println!("[TEST] Step 11/{total}: End episode 2");
    ffmpeg.stop().await;
    tokio::time::sleep(Duration::from_secs(15)).await;
    let ended_two = await_status(&relay, since, &stream_id, "ended").await;
    assert_eq!(address(&ended_two), planned_addr);
    assert_eq!(
        nostr_relay::get_tag_value(&ended_two, "title").as_deref(),
        Some(next_title.as_str())
    );
    wait_for_recording(&h.db, &stream_id, episode_one.as_deref()).await;
    println!("[PASS] Step 11/{total}: ended at the same address");

    // ── Step 12: Re-plan the show — a recurring show's next date ────────
    println!("[TEST] Step 12/{total}: Announce the next episode");
    let next = chrono::Utc::now() + chrono::Duration::days(7);
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": stream_id, "starts": next.to_rfc3339() }))
        .await;
    assert!(status.is_success(), "re-planning the ended show failed ({status})");

    let deadline = tokio::time::Instant::now() + RELAY_WAIT;
    let replanned = loop {
        let ev = await_status(&relay, since, &stream_id, "planned").await;
        if nostr_relay::get_tag_value(&ev, "starts") == Some(next.timestamp().to_string()) {
            break ev;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "re-planned starts never reached the relay"
        );
        tokio::time::sleep(RELAY_POLL).await;
    };
    assert_eq!(
        address(&replanned),
        planned_addr,
        "re-planning moved the show to a different address"
    );
    // The previous broadcast's end time belongs to that broadcast, not the next one
    assert!(
        !nostr_relay::has_tag(&replanned, "ends"),
        "re-planned show still carries the previous broadcast's ends"
    );
    println!("[PASS] Step 12/{total}: announced again at the same address");

    // ── Step 13: The API agrees ────────────────────────────────────────
    println!("[TEST] Step 13/{total}: Read back the re-planned show");
    let final_state = h.client.get_stream(&stream_id).await;
    assert_eq!(final_state["state"].as_str(), Some("planned"));
    assert_eq!(final_state["starts"].as_i64(), Some(next.timestamp()));
    assert!(final_state["ends"].is_null(), "stale ends survived in the database");
    println!("[PASS] Step 13/{total}: Lifecycle complete");
    relay.disconnect().await;
}

/// Rescheduling an announced show must obey the same rules as announcing it, and must
/// republish. A reschedule past the bound key's expiry would leave a show that is
/// advertised as upcoming but can never air.
#[tokio::test]
#[ignore]
async fn e2e_planned_show_reschedule_is_validated_and_republished() {
    let h = boot(false).await;
    let relay = NostrRelay::connect(&h.config.nostr_relay_url).await;
    let since = Timestamp::from(chrono::Utc::now().timestamp() as u64 - 60);

    // Announce with a key that expires two hours after the show starts
    let starts = chrono::Utc::now() + chrono::Duration::hours(1);
    let expires = starts + chrono::Duration::hours(2);
    let title = format!("Reschedule Me {}", h.run_id);
    let created = h
        .client
        .create_key_with(serde_json::json!({
            "event": { "title": title, "tags": ["reschedule", h.run_id] },
            "starts": starts.to_rfc3339(),
            "expires": expires.to_rfc3339(),
        }))
        .await;
    let key = created["key"].as_str().expect("no key").to_string();
    let stream_id = h
        .client
        .list_keys()
        .await
        .as_array()
        .expect("keys is not an array")
        .iter()
        .find(|k| k["key"].as_str() == Some(&key))
        .expect("created key missing from GET /keys")["stream_id"]
        .as_str()
        .expect("no stream_id")
        .to_string();
    let planned = await_status(&relay, since, &stream_id, "planned").await;
    let planned_addr = address(&planned);

    // A reschedule past the key's expiry must be rejected, not silently accepted
    let bad = h
        .client
        .patch_event(serde_json::json!({
            "id": stream_id,
            "starts": (expires + chrono::Duration::hours(1)).to_rfc3339(),
        }))
        .await;
    assert!(
        !bad.is_success(),
        "PATCH accepted a start time past the key's expiry ({bad})"
    );
    assert_eq!(
        h.client.get_stream(&stream_id).await["starts"].as_i64(),
        Some(starts.timestamp()),
        "a rejected reschedule must not have changed the stored start time"
    );

    // ends before starts must be rejected too
    let bad_ends = h
        .client
        .patch_event(serde_json::json!({
            "id": stream_id,
            "ends": (starts - chrono::Duration::minutes(1)).to_rfc3339(),
        }))
        .await;
    assert!(
        !bad_ends.is_success(),
        "PATCH accepted an end before the start ({bad_ends})"
    );

    // A valid reschedule is applied and republished at the same address
    let moved = starts + chrono::Duration::minutes(30);
    let ok = h
        .client
        .patch_event(serde_json::json!({ "id": stream_id, "starts": moved.to_rfc3339() }))
        .await;
    assert!(ok.is_success(), "valid reschedule rejected ({ok})");
    assert_eq!(
        h.client.get_stream(&stream_id).await["starts"].as_i64(),
        Some(moved.timestamp())
    );

    let deadline = tokio::time::Instant::now() + RELAY_WAIT;
    loop {
        let ev = await_status(&relay, since, &stream_id, "planned").await;
        if nostr_relay::get_tag_value(&ev, "starts") == Some(moved.timestamp().to_string()) {
            assert_eq!(
                address(&ev),
                planned_addr,
                "the reschedule moved the show to a different address"
            );
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "rescheduled starts never reached the relay"
        );
        tokio::time::sleep(RELAY_POLL).await;
    }
    println!("[PASS] Reschedule validated and republished");
    relay.disconnect().await;
}

/// Cancelling an announced show must publish a NIP-09 deletion request for its event.
///
/// Whether the 30311 then disappears is up to each relay, so this asserts what the
/// service does, not what the relay chooses to do with it. Cancellation is deliberately
/// soft: the stream row and its key are left alone.
#[tokio::test]
#[ignore]
async fn e2e_cancelling_an_announced_show_publishes_a_deletion_request() {
    let h = boot(false).await;
    let relay = NostrRelay::connect(&h.config.nostr_relay_url).await;
    let since = Timestamp::from(chrono::Utc::now().timestamp() as u64 - 60);

    let starts = chrono::Utc::now() + chrono::Duration::hours(4);
    let created = h
        .client
        .create_key_scheduled(
            &format!("Doomed Show {}", h.run_id),
            "to be cancelled",
            &["cancelme", &h.run_id],
            Some(&starts.to_rfc3339()),
            None,
        )
        .await;
    let key = created["key"].as_str().expect("no key").to_string();
    let stream_id = h
        .client
        .list_keys()
        .await
        .as_array()
        .expect("keys is not an array")
        .iter()
        .find(|k| k["key"].as_str() == Some(&key))
        .expect("created key missing from GET /keys")["stream_id"]
        .as_str()
        .expect("no stream_id")
        .to_string();

    let planned = await_status(&relay, since, &stream_id, "planned").await;
    let event_id = planned.id.to_hex();

    assert!(
        h.client.delete_stream(&stream_id).await.is_success(),
        "DELETE /stream failed"
    );

    // A kind:5 referencing the announced event, by id or by its 30311 coordinate
    let deadline = tokio::time::Instant::now() + RELAY_WAIT;
    loop {
        let found = relay.query_deletions(since).await.into_iter().any(|d| {
            d.tags.iter().any(|t| {
                let v = t.as_slice();
                v.len() >= 2
                    && ((v[0] == "e" && v[1] == event_id)
                        || (v[0] == "a" && v[1].ends_with(&format!(":{stream_id}"))))
            })
        });
        if found {
            break;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no deletion request published for cancelled show {stream_id}"
        );
        tokio::time::sleep(RELAY_POLL).await;
    }

    // Soft by design: nothing else is torn down
    assert_eq!(
        h.db.get_stream_state(&stream_id).await,
        Some(1),
        "cancellation should not change the stream state"
    );
    println!("[PASS] Deletion request published; cancellation left the row alone");
    relay.disconnect().await;
}

/// Look up the stream a custom key is bound to.
async fn stream_id_for_key(h: &Harness, key: &str) -> String {
    h.client
        .list_keys()
        .await
        .as_array()
        .expect("keys is not an array")
        .iter()
        .find(|k| k["key"].as_str() == Some(key))
        .expect("created key missing from GET /keys")["stream_id"]
        .as_str()
        .expect("no stream_id")
        .to_string()
}

/// Only a future `starts` announces a show. A key created without one is not announced,
/// and neither is it by a later edit that doesn't carry one — including a save that
/// round-trips the stored, past `starts`. A future `starts` by PATCH announces it.
#[tokio::test]
#[ignore]
async fn e2e_a_key_is_announced_only_by_a_future_starts() {
    let total = 3;
    let h = boot(false).await;
    let relay = NostrRelay::connect(&h.config.nostr_relay_url).await;
    let since = Timestamp::from(chrono::Utc::now().timestamp() as u64 - 60);

    // ── Step 1: no starts, and a past starts — neither announces ───────
    println!("[TEST] Step 1/{total}: Create keys without a future starts");
    let plain = h
        .client
        .create_key(
            &format!("Plain Key {}", h.run_id),
            "no schedule",
            &["plain", &h.run_id],
        )
        .await;
    assert!(
        plain["event"].is_null(),
        "a key without starts was announced: {plain}"
    );
    let past = h
        .client
        .create_key_with(serde_json::json!({
            "event": { "title": format!("Past Key {}", h.run_id) },
            "starts": (chrono::Utc::now() - chrono::Duration::hours(1)).to_rfc3339(),
        }))
        .await;
    assert!(
        past["event"].is_null(),
        "a key with a past starts was announced: {past}"
    );
    let plain_id = stream_id_for_key(&h, plain["key"].as_str().expect("no key")).await;
    let past_id = stream_id_for_key(&h, past["key"].as_str().expect("no key")).await;
    println!("[PASS] Step 1/{total}: neither key returned an event");

    // ── Step 2: edits without a future starts stay quiet ───────────────
    println!("[TEST] Step 2/{total}: Edit without a future starts");
    let renamed = format!("Renamed {}", h.run_id);
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": plain_id, "title": renamed }))
        .await;
    assert!(status.is_success(), "title edit failed ({status})");
    // a save form that echoes back every field, including the stored starts
    let stored = h.client.get_stream(&plain_id).await["starts"]
        .as_i64()
        .expect("no starts in read-back");
    let stored = chrono::DateTime::from_timestamp(stored, 0)
        .expect("bad timestamp")
        .to_rfc3339();
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": plain_id, "starts": stored }))
        .await;
    assert!(status.is_success(), "round-tripped save failed ({status})");

    tokio::time::sleep(Duration::from_secs(5)).await;
    for d in [&plain_id, &past_id] {
        let events = relay.query_30311_events(since, Some(d)).await;
        assert!(
            events.is_empty(),
            "unannounced key {d} reached the relay ({} event(s))",
            events.len()
        );
    }
    assert!(h.client.get_stream(&plain_id).await["event"].is_null());
    println!("[PASS] Step 2/{total}: nothing reached the relay");

    // ── Step 3: a future starts by PATCH announces it ──────────────────
    println!("[TEST] Step 3/{total}: PATCH a future starts");
    let starts = chrono::Utc::now() + chrono::Duration::hours(3);
    let status = h
        .client
        .patch_event(serde_json::json!({ "id": plain_id, "starts": starts.to_rfc3339() }))
        .await;
    assert!(status.is_success(), "announcing by PATCH failed ({status})");
    let planned = await_status(&relay, since, &plain_id, "planned").await;
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "starts"),
        Some(starts.timestamp().to_string())
    );
    assert_eq!(
        nostr_relay::get_tag_value(&planned, "title").as_deref(),
        Some(renamed.as_str()),
        "the announcement lost the earlier edit"
    );
    println!("[PASS] Step 3/{total}: announced by PATCH");
    relay.disconnect().await;
}

/// Editing an ended stream is for custom-key shows only. A finished account-key stream
/// rejects every edit exactly as before — an ordinary one, and one carrying a future
/// `starts` — because going live on the account key always starts a fresh stream from the
/// account defaults, so the old row is never reused.
#[tokio::test]
#[ignore]
async fn e2e_an_ended_account_key_stream_still_rejects_edits() {
    let h = boot(false).await;
    let stream_id = h
        .db
        .insert_ended_account_stream(&h.client.pubkey_hex())
        .await;

    let status = h
        .client
        .patch_event(serde_json::json!({ "id": stream_id, "title": "edited while offline" }))
        .await;
    assert!(
        !status.is_success(),
        "an ended account-key stream accepted an edit ({status})"
    );

    let status = h
        .client
        .patch_event(serde_json::json!({
            "id": stream_id,
            "starts": (chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339(),
        }))
        .await;
    assert!(
        !status.is_success(),
        "an ended account-key stream accepted a re-plan ({status})"
    );
    assert_eq!(
        h.db.get_stream_state(&stream_id).await,
        Some(3),
        "the account-key stream's state changed"
    );
    println!("[PASS] Ended account-key stream still rejects edits");
}

/// A key that expires before its show airs could never be used, so it is rejected.
#[tokio::test]
#[ignore]
async fn e2e_planned_show_rejects_key_expiring_before_start() {
    let h = boot(false).await;
    let starts = chrono::Utc::now() + chrono::Duration::hours(4);
    let status = h
        .client
        .try_create_key(serde_json::json!({
            "event": { "title": format!("Bad schedule {}", h.run_id) },
            "starts": starts.to_rfc3339(),
            "expires": (starts - chrono::Duration::hours(1)).to_rfc3339(),
        }))
        .await;
    assert!(
        !status.is_success(),
        "POST /keys accepted a key expiring before the show starts ({status})"
    );

    let status = h
        .client
        .try_create_key(serde_json::json!({
            "event": { "title": format!("Bad window {}", h.run_id) },
            "starts": starts.to_rfc3339(),
            "ends": (starts - chrono::Duration::minutes(1)).to_rfc3339(),
        }))
        .await;
    assert!(
        !status.is_success(),
        "POST /keys accepted a show ending before it starts ({status})"
    );
    println!("[PASS] Invalid schedules rejected");
}
