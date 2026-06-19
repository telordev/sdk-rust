//! The feature-flags API (`GET /v1/flags`).

use std::collections::HashMap;

use reqwest::Method;
use serde::Deserialize;

use crate::client::Client;
use crate::error::Result;

/// `GET /v1/flags` envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct FlagSet {
    /// The flag map: `flag name -> enabled`.
    #[serde(default)]
    pub flags: HashMap<String, bool>,
}

impl FlagSet {
    /// Whether a feature flag is enabled (absent → `false`).
    pub fn is_enabled(&self, flag: &str) -> bool {
        self.flags.get(flag).copied().unwrap_or(false)
    }
}

/// The flags resource.
#[derive(Clone)]
pub struct Flags {
    client: Client,
}

impl Flags {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/flags` → the caller's feature-flag map.
    pub async fn get(&self) -> Result<FlagSet> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/flags", None)
            .await
    }
}
