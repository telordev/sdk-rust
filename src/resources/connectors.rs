//! The Connectors API (`/v1/connectors*`) — register remote-MCP servers and
//! attach them to sessions so the agentic loop can call their tools.

use reqwest::Method;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::client::Client;
use crate::error::Result;
use crate::types::{Connector, ConnectorList, ConnectorTestResult};

/// Authentication credentials for a connector.
///
/// Only `"bearer"` kind is currently supported.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ConnectorAuth {
    /// Always `"bearer"`.
    pub kind: String,
    /// The bearer token value. Omitted from API responses (secrets are
    /// redacted server-side, spec §1.3).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// When `true`, the per-session team bearer is used instead of the stored
    /// value; the stored `value` acts only as a fallback.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub per_session: Option<bool>,
}

impl ConnectorAuth {
    /// Construct a bearer-token auth with a static value.
    pub fn bearer(value: impl Into<String>) -> Self {
        Self {
            kind: "bearer".into(),
            value: Some(value.into()),
            per_session: None,
        }
    }

    /// Construct a per-session bearer (the per-session team token is used;
    /// `value` is an optional fallback).
    pub fn per_session() -> Self {
        Self {
            kind: "bearer".into(),
            value: None,
            per_session: Some(true),
        }
    }
}

/// Parameters for creating a connector.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorCreateParams {
    /// A human-readable name for the connector.
    pub name: String,
    /// The connector type. Currently always `"mcp"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// The MCP endpoint URL (the JSON-RPC POST target).
    pub url: String,
    /// Authentication credentials.
    pub auth: ConnectorAuth,
    /// Static extra headers sent on every MCP request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// Allowlist of tool names. When set, only listed tools are exposed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_allowlist: Option<Vec<String>>,
    /// Denylist of tool names. Listed tools are hidden from the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_denylist: Option<Vec<String>>,
}

impl ConnectorCreateParams {
    /// Start building a connector with the required fields.
    pub fn new(
        name: impl Into<String>,
        url: impl Into<String>,
        auth: ConnectorAuth,
    ) -> Self {
        Self {
            name: name.into(),
            kind: "mcp".into(),
            url: url.into(),
            auth,
            headers: None,
            tool_allowlist: None,
            tool_denylist: None,
        }
    }

    /// Add a static request header.
    pub fn header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .get_or_insert_with(Default::default)
            .insert(name.into(), value.into());
        self
    }

    /// Set the tool allowlist.
    pub fn tool_allowlist(mut self, tools: Vec<String>) -> Self {
        self.tool_allowlist = Some(tools);
        self
    }

    /// Set the tool denylist.
    pub fn tool_denylist(mut self, tools: Vec<String>) -> Self {
        self.tool_denylist = Some(tools);
        self
    }
}

/// Parameters for partially updating a connector (`PATCH`).
#[derive(Debug, Clone, Default, Serialize)]
pub struct ConnectorUpdateParams {
    /// New name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New URL.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// New auth credentials.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<ConnectorAuth>,
    /// New static headers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<std::collections::HashMap<String, String>>,
    /// New tool allowlist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_allowlist: Option<Vec<String>>,
    /// New tool denylist.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_denylist: Option<Vec<String>>,
}

/// The connectors resource.
#[derive(Clone)]
pub struct Connectors {
    client: Client,
}

impl Connectors {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `POST /v1/connectors` → `{ id }` (201).
    pub async fn create(&self, params: ConnectorCreateParams) -> Result<Connector> {
        self.client
            .request_json(Method::POST, "/v1/connectors", Some(&params))
            .await
    }

    /// `GET /v1/connectors` → list of connectors (secrets redacted).
    pub async fn list(&self) -> Result<ConnectorList> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/connectors", None)
            .await
    }

    /// `GET /v1/connectors/{id}` → one connector (secrets redacted).
    pub async fn retrieve(&self, id: impl AsRef<str>) -> Result<Connector> {
        let path = format!("/v1/connectors/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `PATCH /v1/connectors/{id}` → updated connector (partial update).
    pub async fn update(
        &self,
        id: impl AsRef<str>,
        params: ConnectorUpdateParams,
    ) -> Result<Connector> {
        let path = format!("/v1/connectors/{}", id.as_ref());
        self.client.request_json(Method::PATCH, &path, Some(&params)).await
    }

    /// `DELETE /v1/connectors/{id}` → `{ deleted: true }`.
    pub async fn delete(&self, id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/connectors/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }

    /// `POST /v1/connectors/{id}/test` → live `tools/list` probe.
    ///
    /// Returns `{ ok, tool_count, error? }`. A non-`ok` result is still a 200
    /// response — the test reports rather than fails the request.
    pub async fn test(&self, id: impl AsRef<str>) -> Result<ConnectorTestResult> {
        let path = format!("/v1/connectors/{}/test", id.as_ref());
        self.client.request_json::<(), _>(Method::POST, &path, None).await
    }
}
