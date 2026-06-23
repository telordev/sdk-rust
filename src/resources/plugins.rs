//! The Plugins / marketplace API (`/v1/plugins*`).

use std::collections::HashMap;

use reqwest::Method;
use serde::Serialize;
use serde_json::Value;

use crate::client::Client;
use crate::error::Result;

/// The plugins resource.
#[derive(Clone)]
pub struct Plugins {
    client: Client,
}

/// Parameters for installing a plugin.
#[derive(Debug, Clone, Serialize)]
pub struct InstallParams {
    /// The plugin name to install.
    pub plugin_name: String,
    /// Permissions to grant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<Vec<String>>,
    /// Secret values to provision (key → value).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secrets: Option<HashMap<String, String>>,
}

impl InstallParams {
    /// Install by name.
    pub fn new(plugin_name: impl Into<String>) -> Self {
        Self {
            plugin_name: plugin_name.into(),
            permissions: None,
            secrets: None,
        }
    }
    /// Grant permissions.
    pub fn permissions(mut self, perms: Vec<String>) -> Self {
        self.permissions = Some(perms);
        self
    }
    /// Provision secrets.
    pub fn secrets(mut self, secrets: HashMap<String, String>) -> Self {
        self.secrets = Some(secrets);
        self
    }
}

impl Plugins {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/plugins` → built-in tools + the caller's installed plugins.
    pub async fn list(&self) -> Result<Value> {
        self.client.request_json::<(), _>(Method::GET, "/v1/plugins", None).await
    }

    /// `GET /v1/plugins/registry` → the marketplace catalog.
    pub async fn registry(&self) -> Result<Value> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/plugins/registry", None)
            .await
    }

    /// `GET /v1/plugins/registry/{id}` → one registry entry (manifest + readme).
    pub async fn registry_detail(&self, id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/plugins/registry/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `GET /v1/plugins/installed` → the caller's installed plugins.
    pub async fn installed(&self) -> Result<Value> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/plugins/installed", None)
            .await
    }

    /// `POST /v1/plugins/install` → install a plugin.
    pub async fn install(&self, params: InstallParams) -> Result<Value> {
        self.client
            .request_json(Method::POST, "/v1/plugins/install", Some(&params))
            .await
    }

    /// `POST /v1/plugins/uninstall` → uninstall a plugin.
    pub async fn uninstall(&self, plugin_name: impl AsRef<str>) -> Result<Value> {
        let body = serde_json::json!({ "plugin_name": plugin_name.as_ref() });
        self.client
            .request_json(Method::POST, "/v1/plugins/uninstall", Some(&body))
            .await
    }

    /// `POST /v1/plugins/enabled` → enable or disable an installed plugin.
    pub async fn set_enabled(&self, plugin_name: impl AsRef<str>, enabled: bool) -> Result<Value> {
        let body = serde_json::json!({ "plugin_name": plugin_name.as_ref(), "enabled": enabled });
        self.client
            .request_json(Method::POST, "/v1/plugins/enabled", Some(&body))
            .await
    }
}
