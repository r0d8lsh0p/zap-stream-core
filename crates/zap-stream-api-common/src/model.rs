use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize)]
pub struct AccountInfo {
    pub endpoints: Vec<Endpoint>,
    pub balance: i64,
    pub tos: AccountTos,
    pub forwards: Vec<ForwardDest>,
    pub details: Option<PatchEventDetails>,
    pub has_nwc: bool,
}

#[derive(Deserialize, Serialize)]
pub struct Endpoint {
    pub name: String,
    pub url: String,
    pub key: String,
    pub capabilities: Vec<String>,
    pub cost: EndpointCost,
}

#[derive(Deserialize, Serialize)]
pub struct EndpointCost {
    pub unit: String,
    pub rate: f32,
}

#[derive(Deserialize, Serialize)]
pub struct AccountTos {
    pub accepted: bool,
    pub link: String,
}

#[derive(Deserialize, Serialize)]
pub struct PatchAccount {
    /// Accept TOS
    pub accept_tos: Option<bool>,
    /// Configure a new NWC
    pub nwc: Option<String>,
    /// Remove configured NWC
    pub remove_nwc: Option<bool>,
}

#[derive(Deserialize, Serialize)]
pub struct TopupResponse {
    pub pr: String,
}

#[cfg(feature = "withdrawal")]
#[derive(Deserialize, Serialize)]
pub struct WithdrawRequest {
    pub payment_request: String,
    pub amount: u64,
}

#[cfg(feature = "withdrawal")]
#[derive(Deserialize, Serialize)]
pub struct WithdrawResponse {
    pub fee: i64,
    pub preimage: String,
}

#[derive(Deserialize, Serialize)]
pub struct ForwardRequest {
    pub name: String,
    pub target: String,
}

#[derive(Deserialize, Serialize)]
pub struct ForwardResponse {
    pub id: u64,
}

#[derive(Deserialize, Serialize)]
pub struct UpdateForwardRequest {
    pub disabled: bool,
}

#[derive(Deserialize, Serialize)]
pub struct HistoryEntry {
    pub created: u64,
    #[serde(rename = "type")]
    pub entry_type: i32,
    pub amount: f64,
    pub desc: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct HistoryResponse {
    pub items: Vec<HistoryEntry>,
    pub page: i32,
    pub page_size: i32,
}

#[derive(Deserialize, Serialize)]
pub struct StreamKey {
    pub id: u64,
    pub key: String,
    pub created: i64,
    pub expires: Option<i64>,
    pub stream_id: String,
    /// The stream (show) this key is bound to, so clients can render the
    /// event details they created the key with without a second request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<StreamInfo>,
}

/// Read-back of a user's own stream event.
#[derive(Deserialize, Serialize, Clone)]
pub struct StreamInfo {
    pub id: String,
    /// One of `unknown`, `planned`, `live`, `ended`
    pub state: String,
    pub starts: i64,
    pub ends: Option<i64>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub image: Option<String>,
    pub thumb: Option<String>,
    pub tags: Option<Vec<String>>,
    pub content_warning: Option<String>,
    pub goal: Option<String>,
    /// The published kind:30311 event JSON, if one has been published
    pub event: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct CreateStreamKeyRequest {
    pub event: PatchEventDetails,
    pub expires: Option<DateTime<Utc>>,
    /// When the show starts. Defaults to now, set it in the future to
    /// announce a planned show.
    pub starts: Option<DateTime<Utc>>,
    /// When the show is expected to end.
    pub ends: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize)]
pub struct CreateStreamKeyResponse {
    pub key: String,
    pub event: Option<String>,
}

#[derive(Deserialize, Serialize, Clone)]
pub struct PatchEvent {
    pub id: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub image: Option<String>,
    pub tags: Option<Vec<String>>,
    pub content_warning: Option<String>,
    pub goal: Option<String>,
    /// Reschedule a planned show. Ignored when patching user defaults.
    pub starts: Option<DateTime<Utc>>,
    pub ends: Option<DateTime<Utc>>,
}

#[derive(Deserialize, Serialize, Clone, Default)]
pub struct PatchEventDetails {
    pub title: Option<String>,
    pub summary: Option<String>,
    pub image: Option<String>,
    pub tags: Option<Vec<String>>,
    pub content_warning: Option<String>,
    pub goal: Option<String>,
}

#[derive(Deserialize, Serialize)]
pub struct ForwardDest {
    pub id: u64,
    pub name: String,
    pub disabled: bool,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct GameInfo {
    pub id: u64,
    pub name: String,
    pub summary: Option<String>,
    #[serde(default)]
    pub genres: Vec<GameGenre>,
    #[serde(default)]
    pub cover: Option<GameCover>,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct GameGenre {
    pub id: u64,
    pub name: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct GameCover {
    pub id: u64,
    pub image_id: String,
}
