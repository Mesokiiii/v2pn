//! Subscription handling.
//!
//! High-level flow:
//! 1. [`fetch::fetch_subscription`] downloads the URL with a "client-like"
//!    User-Agent, returns raw bytes + relevant headers.
//! 2. [`format::detect`] sniffs the body and decides which format we got.
//! 3. The appropriate parser (`uri_list`, `singbox`, `clash`, `base64`)
//!    converts the body into `Vec<ProxyProfile>` plus subscription metadata.
//! 4. [`html`] handles "happ-aware" HTML landing pages — extracts deep-link
//!    URLs and probes convention endpoints to find the real raw payload.

pub mod base64;
pub mod clash;
pub mod fetch;
pub mod format;
pub mod html;
pub mod meta;
pub mod singbox;
pub mod uri;
pub mod uri_list;
pub mod xray_array;

use serde::{Deserialize, Serialize};

use crate::profile::ProxyProfile;

/// Result of parsing a subscription response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedSubscription {
    pub profiles: Vec<ProxyProfile>,
    pub meta: meta::SubscriptionMeta,
}
