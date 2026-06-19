//! The Memories API (`/v1/memories*`).

use std::collections::HashMap;

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::client::Client;
use crate::error::Result;
use crate::types::{CreatedMemory, MemoryList, MemoryStats};

/// The memories resource.
#[derive(Clone)]
pub struct Memories {
    client: Client,
}

/// Parameters for creating a memory.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryCreateParams {
    /// The memory text.
    pub text: String,
    /// Optional string metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, String>>,
}

impl MemoryCreateParams {
    /// A memory from text alone.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            metadata: None,
        }
    }

    /// Attach metadata.
    pub fn with_metadata(mut self, metadata: HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Memories {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/memories` → list (recency order).
    pub async fn list(&self) -> Result<MemoryList> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/memories", None)
            .await
    }

    /// `GET /v1/memories?query=&limit=` → semantic search.
    pub async fn search(&self, query: impl AsRef<str>, limit: Option<u32>) -> Result<MemoryList> {
        let mut path = format!("/v1/memories?query={}", urlencode(query.as_ref()));
        if let Some(l) = limit {
            path.push_str(&format!("&limit={l}"));
        }
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `POST /v1/memories` → create. Returns the new id.
    pub async fn create(&self, params: MemoryCreateParams) -> Result<CreatedMemory> {
        self.client
            .request_json(Method::POST, "/v1/memories", Some(&params))
            .await
    }

    /// `DELETE /v1/memories/{id}` → delete.
    pub async fn delete(&self, id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/memories/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }

    /// `GET /v1/memories/stats` → counts.
    pub async fn stats(&self) -> Result<MemoryStats> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/memories/stats", None)
            .await
    }
}

/// Minimal percent-encoding for a query value (encodes the characters that would
/// break a query string; the gateway tolerates the rest).
fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query() {
        assert_eq!(urlencode("hello world"), "hello%20world");
        assert_eq!(urlencode("a&b=c"), "a%26b%3Dc");
        assert_eq!(urlencode("plain"), "plain");
    }
}
