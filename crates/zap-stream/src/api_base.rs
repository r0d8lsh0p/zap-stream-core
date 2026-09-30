use crate::user_history_to_api_model;
use anyhow::Result;
use anyhow::bail;
use chrono::{DateTime, Utc};
use nostr_sdk::Client;
use nostr_sdk::prelude::{EventDeletionRequest, NostrWalletConnectUri};
use nwc::NostrWalletConnect;
use payments_rs::lightning::{AddInvoiceRequest, LightningNode};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};
use uuid::Uuid;
use zap_stream_api_common::{
    CreateStreamKeyRequest, HistoryEntry, HistoryResponse, Nip98Auth, PatchAccount, PatchEvent,
    StreamInfo, StreamKey, TopupResponse,
};
use zap_stream_db::{UserStream, UserStreamState, ZapStreamDb};

/// Convert a list of tags into a comma-separated CSV string.
/// Filters out empty/whitespace-only tags, and returns None if no tags remain.
pub(crate) fn tags_to_csv(tags: Vec<String>) -> Option<String> {
    let filtered: Vec<String> = tags
        .into_iter()
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if filtered.is_empty() {
        None
    } else {
        Some(filtered.join(","))
    }
}

/// Split a comma-separated tag list back into individual tags.
pub(crate) fn csv_to_tags(csv: &str) -> Option<Vec<String>> {
    let out: Vec<String> = csv
        .split(',')
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .collect();
    if out.is_empty() { None } else { Some(out) }
}

/// Validate a show's schedule. `expires` is the bound stream key's expiry, if any.
///
/// Pure so that both the create and the reschedule path can share it — they used to
/// disagree, and a reschedule could push `starts` past the key's expiry and leave the
/// show permanently unstreamable while still advertised as planned.
pub(crate) fn validate_schedule(
    starts: DateTime<Utc>,
    ends: Option<DateTime<Utc>>,
    expires: Option<DateTime<Utc>>,
) -> Result<()> {
    if let Some(ends) = ends
        && ends <= starts
    {
        bail!("Stream cannot end before it starts");
    }
    // A key that expires before the show airs could never be used
    if let Some(expires) = expires
        && expires <= starts
    {
        bail!("Stream key expires before the stream starts");
    }
    Ok(())
}

/// A `starts` only counts when it is in the future: that is what makes a request an
/// announcement. A past or present value — such as a stored `starts` echoed back by a
/// client that round-trips every field — is treated as absent.
pub(crate) fn future_starts(
    starts: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    starts.filter(|s| *s > now)
}

/// Whether a PATCH carrying a future `starts` re-plans this stream.
///
/// Only custom-key shows can be re-planned. Going live on the account key always starts
/// a fresh stream, so a re-planned account-key stream could never air. A live show is
/// never re-planned.
pub(crate) fn replans(stream: &UserStream, future_starts: Option<DateTime<Utc>>) -> bool {
    future_starts.is_some()
        && stream.stream_key_id.is_some()
        && stream.state != UserStreamState::Live
}

/// Whether an edit should publish the stream's event: when it announces the show, and to
/// keep anything already on the network accurate. Not for an ended show — its event
/// belongs to its previous broadcast, so an edit is saved for the next go-live to publish.
/// A key that was never announced stays quiet.
pub(crate) fn edit_publishes(stream: &UserStream, replan: bool) -> bool {
    replan
        || stream.state == UserStreamState::Live
        || (stream.event.is_some() && stream.state != UserStreamState::Ended)
}

/// Map a stream row into the read-back API model.
pub fn stream_to_info(stream: &UserStream) -> StreamInfo {
    StreamInfo {
        id: stream.id.clone(),
        state: stream.state.to_string(),
        starts: stream.starts.timestamp(),
        ends: stream.ends.map(|e| e.timestamp()),
        title: stream.title.clone(),
        summary: stream.summary.clone(),
        image: stream.image.clone(),
        thumb: stream.thumb.clone(),
        tags: stream.tags.as_deref().and_then(csv_to_tags),
        content_warning: stream.content_warning.clone(),
        goal: stream.goal.clone(),
        event: stream.event.clone(),
    }
}

/// Basic API implementation which covers the simple database updates
#[derive(Clone)]
pub struct ApiBase {
    db: ZapStreamDb,
    client: Client,
    lightning: Arc<dyn LightningNode>,
}

impl ApiBase {
    pub fn new(db: ZapStreamDb, client: Client, lightning: Arc<dyn LightningNode>) -> Self {
        Self {
            db,
            client,
            lightning,
        }
    }

    pub async fn update_account(&self, auth: Nip98Auth, patch_account: PatchAccount) -> Result<()> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;

        if let Some(accept_tos) = patch_account.accept_tos
            && accept_tos
        {
            let user = self.db.get_user(uid).await?;
            if user.tos_accepted.is_none() {
                self.db.accept_tos(uid).await?;
            }
        }

        if let Some(url) = patch_account.nwc
            && patch_account.remove_nwc.is_none()
        {
            // test connection
            let parsed = NostrWalletConnectUri::parse(&url)?;
            let nwc = NostrWalletConnect::new(parsed);
            let info = nwc.get_info().await?;
            if !info.methods.contains(&nwc::prelude::Method::PayInvoice) {
                bail!("NWC connection does not allow paying invoices!");
            }
            self.db.update_user_nwc(uid, Some(&url)).await?;
        }

        if let Some(x) = patch_account.remove_nwc
            && x
        {
            self.db.update_user_nwc(uid, None).await?;
        }

        Ok(())
    }

    /// Apply a metadata patch. Returns the stream when its nostr event should be
    /// published, which the caller then does with its own publish primitive.
    pub async fn update_event(
        &self,
        auth: Nip98Auth,
        patch: PatchEvent,
    ) -> Result<Option<UserStream>> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;

        if patch.id.as_ref().map(|i| !i.is_empty()).unwrap_or(false) {
            // Update specific stream
            let stream_uuid = Uuid::parse_str(&patch.id.unwrap())?;
            let mut stream = self.db.get_stream(&stream_uuid).await?;

            // Verify user owns this stream
            if stream.user_id != uid {
                bail!("Unauthorized: Stream belongs to different user");
            }

            let starts = future_starts(patch.starts, Utc::now());
            let replan = replans(&stream, starts);

            // Don't allow modifications of ended streams — except a custom-key show,
            // whose row its next go-live reuses, so this is how the next episode gets
            // its details. The account key always starts a fresh stream from the account
            // defaults, so it is unaffected.
            if stream.state == UserStreamState::Ended && stream.stream_key_id.is_none() {
                bail!("Cannot modify ended stream");
            }

            // Update stream with patch data
            if let Some(title) = patch.title {
                stream.title = Some(title);
            }
            if let Some(summary) = patch.summary {
                stream.summary = Some(summary);
            }
            if let Some(image) = patch.image {
                stream.image = Some(image);
            }
            if let Some(tags) = patch.tags {
                stream.tags = tags_to_csv(tags);
            }
            if let Some(content_warning) = patch.content_warning {
                stream.content_warning = Some(content_warning);
            }
            if let Some(goal) = patch.goal {
                stream.goal = Some(goal);
            }
            if let Some(starts) = starts
                && replan
            {
                // A finished show's `ends` belongs to its previous broadcast
                let ends = if patch.ends.is_some() {
                    patch.ends
                } else if stream.state == UserStreamState::Ended {
                    None
                } else {
                    stream.ends
                };
                validate_schedule(starts, ends, self.key_expiry(&stream).await?)?;
                stream.starts = starts;
                stream.ends = ends;
                stream.state = UserStreamState::Planned;
            } else if stream.state == UserStreamState::Planned && patch.ends.is_some() {
                validate_schedule(stream.starts, patch.ends, self.key_expiry(&stream).await?)?;
                stream.ends = patch.ends;
            }

            let publish = edit_publishes(&stream, replan);
            self.db.update_stream(&stream).await?;
            return Ok(publish.then_some(stream));
        } else {
            // Update user default stream info
            self.db
                .update_user_defaults(
                    uid,
                    patch.title.as_deref(),
                    patch.summary.as_deref(),
                    patch.image.as_deref(),
                    patch.tags.as_ref().and_then(|t| tags_to_csv(t.clone())).as_deref(),
                    patch.content_warning.as_deref(),
                    patch.goal.as_deref(),
                )
                .await?;
        }
        Ok(None)
    }

    /// Expiry of the custom key bound to a stream, if any. The key must still be
    /// valid at the scheduled time, or the show could never air.
    async fn key_expiry(&self, stream: &UserStream) -> Result<Option<DateTime<Utc>>> {
        Ok(match stream.stream_key_id {
            Some(key_id) => self.db.get_user_stream_key_by_id(key_id).await?.expires,
            None => None,
        })
    }

    pub async fn delete_event(&self, auth: Nip98Auth, stream_id: Uuid) -> Result<()> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;
        let stream = self.db.get_stream(&stream_id).await?;

        // Verify the user owns this stream OR is an admin
        let is_admin = self.db.is_admin(uid).await?;
        if stream.user_id != uid && !is_admin {
            bail!("Access denied: You can only delete your own streams");
        }

        // Publish Nostr deletion request event if the stream has an associated event
        if let Some(event_json) = &stream.event
            && let Ok(stream_event) = serde_json::from_str::<nostr_sdk::Event>(event_json)
        {
            let deletion_event = nostr_sdk::EventBuilder::delete(
                EventDeletionRequest::new()
                    .id(stream_event.id)
                    .coordinate(stream_event.coordinate().unwrap().into_owned()),
            );

            if let Err(e) = self.client.send_event_builder(deletion_event).await {
                warn!(
                    "Failed to publish deletion event for stream {}: {}",
                    stream_id, e
                );
            } else {
                info!("Published deletion request event for stream {}", stream_id);
            }
        }

        // Log admin action if this is an admin deleting someone else's stream
        if is_admin && stream.user_id != uid {
            let message = format!(
                "Admin deleted stream {} belonging to user {}",
                stream_id, stream.user_id
            );
            let metadata = serde_json::json!({
                "target_stream_id": stream_id,
                "target_user_id": stream.user_id,
                "stream_title": stream.title
            });
            self.db
                .log_admin_action(
                    uid,
                    "delete_stream",
                    Some("stream"),
                    Some(&stream_id.to_string()),
                    &message,
                    Some(&metadata.to_string()),
                )
                .await?;
        }

        Ok(())
    }

    pub async fn get_balance_history(
        &self,
        auth: Nip98Auth,
        page: u32,
        page_size: u32,
    ) -> Result<HistoryResponse> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;
        let offset = page * page_size;
        let history_entries = self
            .db
            .get_unified_user_history(uid, offset as _, page_size as _)
            .await?;

        let items: Vec<HistoryEntry> = history_entries
            .into_iter()
            .map(user_history_to_api_model)
            .collect();

        Ok(HistoryResponse {
            items,
            page: page as i32,
            page_size: page_size as i32,
        })
    }

    /// Look up the stream rows bound to a user's custom keys, indexed by stream id.
    async fn keyed_stream_index(&self, uid: u64) -> Result<HashMap<String, UserStream>> {
        Ok(self
            .db
            .get_user_keyed_streams(uid)
            .await?
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect())
    }

    pub async fn get_stream_keys(&self, auth: Nip98Auth) -> Result<Vec<StreamKey>> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;
        let keys = self.db.get_user_stream_keys(uid).await?;
        let streams = self.keyed_stream_index(uid).await?;

        Ok(keys
            .into_iter()
            .map(|k| StreamKey {
                id: k.id,
                created: k.created.timestamp(),
                expires: k.expires.map(|e| e.timestamp()),
                stream: streams.get(&k.stream_id).map(stream_to_info),
                stream_id: k.stream_id,
                key: k.key,
            })
            .collect())
    }

    /// Attach the stream details to a list of keys built by a backend that
    /// substitutes its own key values (e.g. Cloudflare Live Input keys).
    pub async fn attach_stream_info(&self, uid: u64, keys: &mut [StreamKey]) -> Result<()> {
        let streams = self.keyed_stream_index(uid).await?;
        for key in keys.iter_mut() {
            key.stream = streams.get(&key.stream_id).map(stream_to_info);
        }
        Ok(())
    }

    pub async fn get_stream_info(&self, auth: Nip98Auth, stream_id: Uuid) -> Result<StreamInfo> {
        let uid = self.db.upsert_user(&auth.pubkey).await?;
        let stream = self.db.get_stream(&stream_id).await?;
        if stream.user_id != uid && !self.db.is_admin(uid).await? {
            bail!("Access denied: You can only read your own streams");
        }
        Ok(stream_to_info(&stream))
    }

    /// Create the planned stream row for a new custom stream key, plus the key
    /// row itself, and link the two. Shared by every backend so that the
    /// scheduling rules live in exactly one place.
    ///
    /// `key` is the value the ingest server will receive; `external_id` is the
    /// backend's own handle for it (e.g. a Cloudflare Live Input UID).
    ///
    /// Also returns whether the request announced the show — it did if it carried
    /// a future `starts` — in which case the caller publishes it.
    pub async fn create_keyed_stream(
        &self,
        uid: u64,
        key: &str,
        external_id: Option<&str>,
        req: &CreateStreamKeyRequest,
    ) -> Result<(UserStream, bool)> {
        let now = Utc::now();
        let announced = future_starts(req.starts, now);
        let starts = announced.unwrap_or(now);
        validate_schedule(starts, req.ends, req.expires)?;

        let stream_id = Uuid::new_v4();
        let mut new_stream = UserStream {
            id: stream_id.to_string(),
            user_id: uid,
            starts,
            ends: req.ends,
            state: UserStreamState::Planned,
            title: req.event.title.clone(),
            summary: req.event.summary.clone(),
            image: req.event.image.clone(),
            tags: req.event.tags.clone().and_then(tags_to_csv),
            content_warning: req.event.content_warning.clone(),
            goal: req.event.goal.clone(),
            ..Default::default()
        };
        self.db.insert_stream(&new_stream).await?;

        let key_id = self
            .db
            .create_stream_key(uid, key, external_id, req.expires, &new_stream.id)
            .await?;

        // link the key back onto the stream so going live can find it
        new_stream.stream_key_id = Some(key_id);
        self.db.update_stream(&new_stream).await?;

        Ok((new_stream, announced.is_some()))
    }

    pub async fn topup(
        &self,
        pubkey: [u8; 32],
        amount_msats: u64,
        zap: Option<String>,
    ) -> Result<TopupResponse> {
        let uid = self.db.upsert_user(&pubkey).await?;

        let response = self
            .lightning
            .add_invoice(AddInvoiceRequest {
                amount: amount_msats as _,
                memo: Some(format!("zap.stream topup for user {}", hex::encode(pubkey))),
                expire: None,
            })
            .await?;

        let pr = response.pr();
        let r_hash = hex::decode(response.payment_hash())?;
        // Create payment entry for this topup invoice
        self.db
            .create_payment(
                &r_hash,
                uid,
                Some(&response.pr()),
                amount_msats as _,
                zap_stream_db::PaymentType::TopUp,
                0,
                response
                    .parsed_invoice
                    .expires_at()
                    .and_then(|e| DateTime::from_timestamp(e.as_secs() as _, 0))
                    .unwrap_or_else(|| Utc::now() + chrono::Duration::hours(1)),
                zap,
                response.external_id,
            )
            .await?;

        Ok(TopupResponse { pr })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        csv_to_tags, edit_publishes, future_starts, replans, stream_to_info, tags_to_csv,
        validate_schedule,
    };
    use chrono::{TimeZone, Utc};
    use zap_stream_db::{UserStream, UserStreamState};

    #[test]
    fn edits_publish_what_is_on_the_network_except_an_ended_show() {
        let mut stream = UserStream::default();
        stream.stream_key_id = Some(1);

        stream.state = UserStreamState::Live;
        assert!(
            edit_publishes(&stream, false),
            "a live show is kept accurate"
        );

        stream.state = UserStreamState::Planned;
        assert!(
            !edit_publishes(&stream, false),
            "a never-announced key stays quiet"
        );
        stream.event = Some("{}".to_string());
        assert!(
            edit_publishes(&stream, false),
            "an announced show is kept accurate"
        );

        // The ended event belongs to the previous broadcast: an edit is saved for the
        // next go-live, not published over the top of it
        stream.state = UserStreamState::Ended;
        assert!(!edit_publishes(&stream, false));
        assert!(edit_publishes(&stream, true), "re-planning announces");
    }

    #[test]
    fn only_a_future_starts_announces() {
        let now = Utc::now();
        let later = now + chrono::Duration::hours(1);
        assert_eq!(future_starts(Some(later), now), Some(later));
        assert_eq!(future_starts(None, now), None);
        // a round-tripped creation time, or "now", is not an announcement
        assert_eq!(future_starts(Some(now), now), None);
        assert_eq!(
            future_starts(Some(now - chrono::Duration::hours(1)), now),
            None
        );
    }

    #[test]
    fn only_custom_key_shows_that_are_not_live_can_be_replanned() {
        let later = Some(Utc::now() + chrono::Duration::hours(1));
        let mut stream = UserStream::default();

        stream.stream_key_id = Some(1);
        stream.state = UserStreamState::Planned;
        assert!(replans(&stream, later));
        stream.state = UserStreamState::Ended;
        assert!(
            replans(&stream, later),
            "a recurring show can be announced again"
        );
        assert!(
            !replans(&stream, None),
            "no future starts, nothing to announce"
        );
        stream.state = UserStreamState::Live;
        assert!(!replans(&stream, later), "a live show is never re-planned");

        // the account key is never re-planned, whatever its state
        stream.stream_key_id = None;
        for state in [
            UserStreamState::Planned,
            UserStreamState::Live,
            UserStreamState::Ended,
        ] {
            stream.state = state;
            assert!(!replans(&stream, later));
        }
    }

    #[test]
    fn schedule_must_end_after_it_starts() {
        let starts = Utc::now();
        assert!(validate_schedule(starts, None, None).is_ok());
        assert!(validate_schedule(starts, Some(starts + chrono::Duration::hours(1)), None).is_ok());
        assert!(validate_schedule(starts, Some(starts), None).is_err());
        assert!(
            validate_schedule(starts, Some(starts - chrono::Duration::minutes(1)), None).is_err()
        );
    }

    #[test]
    fn schedule_rejects_a_key_that_expires_before_the_show() {
        let starts = Utc::now();
        assert!(validate_schedule(starts, None, Some(starts + chrono::Duration::hours(1))).is_ok());
        assert!(validate_schedule(starts, None, Some(starts)).is_err());
        assert!(
            validate_schedule(starts, None, Some(starts - chrono::Duration::hours(1))).is_err()
        );
    }

    #[test]
    fn tags_round_trip_through_csv() {
        let tags = vec!["  gaming ".to_string(), String::new(), "nostr".to_string()];
        let csv = tags_to_csv(tags).unwrap();
        assert_eq!(csv, "gaming,nostr");
        assert_eq!(
            csv_to_tags(&csv),
            Some(vec!["gaming".to_string(), "nostr".to_string()])
        );
        assert_eq!(csv_to_tags(" , "), None);
    }

    #[test]
    fn stream_to_info_exposes_the_planned_schedule() {
        let starts = Utc.timestamp_opt(1_800_000_000, 0).unwrap();
        let mut stream = UserStream::default();
        stream.id = "abc".to_string();
        stream.state = UserStreamState::Planned;
        stream.starts = starts;
        stream.ends = Some(starts + chrono::Duration::hours(2));
        stream.title = Some("Upcoming".to_string());
        stream.tags = Some("a,b".to_string());

        let info = stream_to_info(&stream);
        assert_eq!(info.state, "planned");
        assert_eq!(info.starts, starts.timestamp());
        assert_eq!(
            info.ends,
            Some((starts + chrono::Duration::hours(2)).timestamp())
        );
        assert_eq!(info.title.as_deref(), Some("Upcoming"));
        assert_eq!(info.tags, Some(vec!["a".to_string(), "b".to_string()]));
        assert!(info.event.is_none());
    }
}
