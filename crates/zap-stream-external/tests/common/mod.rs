// Each test binary links its own copy of this module and uses a different subset
// of the helpers, so unused ones are expected rather than dead.
#![allow(dead_code)]

pub mod api_client;
pub mod config;
pub mod db;
pub mod docker;
pub mod ffmpeg;
pub mod nostr_relay;
