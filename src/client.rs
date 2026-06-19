//! The HTTP client: construction, auth headers, retries with backoff, the
//! request executor, and the resource-namespace accessors.

use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use futures::Stream;
use reqwest::Method;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::auth::SharedAuthProvider;
use crate::error::{is_retryable_status, Error, Result};
use crate::resources::{
    Account, Agents, Billing, Flags, Memories, Messages, Models, Pm, Sessions, Usage,
};

/// The default API base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.simse.dev";
/// The anthropic-version header value the SDK pins.
pub const ANTHROPIC_VERSION: &str = "2026-06-01";
/// The default request timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
/// The default maximum number of retries (the request counts as the 0th try).
pub const DEFAULT_MAX_RETRIES: u32 = 2;

/// Rate-limit metadata surfaced from the `anthropic-ratelimit-*` response
/// headers (plus the `request-id`).
#[derive(Debug, Clone, Default)]
pub struct ResponseMeta {
    /// The `request-id` header.
    pub request_id: Option<String>,
    /// `anthropic-ratelimit-requests-limit`.
    pub requests_limit: Option<u64>,
    /// `anthropic-ratelimit-requests-remaining`.
    pub requests_remaining: Option<u64>,
    /// `anthropic-ratelimit-requests-reset` (RFC 3339 UTC).
    pub requests_reset: Option<String>,
    /// `retry-after` seconds, when present (429s).
    pub retry_after: Option<u64>,
}

impl ResponseMeta {
    fn from_headers(headers: &reqwest::header::HeaderMap) -> Self {
        let s = |k: &str| {
            headers
                .get(k)
                .and_then(|v| v.to_str().ok())
                .map(str::to_string)
        };
        let n = |k: &str| s(k).and_then(|v| v.parse::<u64>().ok());
        Self {
            request_id: s("request-id"),
            requests_limit: n("anthropic-ratelimit-requests-limit"),
            requests_remaining: n("anthropic-ratelimit-requests-remaining"),
            requests_reset: s("anthropic-ratelimit-requests-reset"),
            retry_after: n("retry-after"),
        }
    }
}

/// A hook invoked with the [`ResponseMeta`] of every API response (success or
/// error). Use it to surface the `request-id` / rate-limit budget.
pub type ResponseHook = Arc<dyn Fn(&ResponseMeta) + Send + Sync>;

/// Internal shared client state.
pub(crate) struct Inner {
    pub(crate) http: reqwest::Client,
    pub(crate) base_url: String,
    pub(crate) api_key: String,
    pub(crate) anthropic_version: String,
    pub(crate) max_retries: u32,
    pub(crate) default_headers: Vec<(String, String)>,
    pub(crate) response_hook: Option<ResponseHook>,
    /// When set, a fresh token is fetched per request and used for both the
    /// `x-api-key` and the bearer value (instead of the static `api_key`).
    pub(crate) auth_provider: Option<SharedAuthProvider>,
}

/// The Simse API client. Cheap to clone (an `Arc` internally).
#[derive(Clone)]
pub struct Client {
    pub(crate) inner: Arc<Inner>,
}

impl Client {
    /// Construct a client with an explicit API key and otherwise-default
    /// configuration. To read the key from the environment instead, use
    /// [`Client::from_env`].
    pub fn new(api_key: impl Into<String>) -> Result<Self> {
        ClientBuilder::new().api_key(api_key).build()
    }

    /// Construct a client, taking the API key + base URL from the environment
    /// (`SIMSE_API_KEY` then `ANTHROPIC_API_KEY`; `SIMSE_BASE_URL`).
    pub fn from_env() -> Result<Self> {
        ClientBuilder::new().build()
    }

    /// Start a [`ClientBuilder`].
    pub fn builder() -> ClientBuilder {
        ClientBuilder::new()
    }

    // ── Resource namespaces ────────────────────────────────────────────────

    /// The Messages API (`create` / `stream` / `count_tokens`).
    pub fn messages(&self) -> Messages {
        Messages::new(self.clone())
    }
    /// The Models API (`list` / `retrieve`).
    pub fn models(&self) -> Models {
        Models::new(self.clone())
    }
    /// The Agents API (`GET /v1/agents`) — subagent run history.
    pub fn agents(&self) -> Agents {
        Agents::new(self.clone())
    }
    /// The account view (`GET /v1/account`).
    pub fn account(&self) -> Account {
        Account::new(self.clone())
    }
    /// The usage view (`GET /v1/usage`).
    pub fn usage(&self) -> Usage {
        Usage::new(self.clone())
    }
    /// The billing view (`GET /v1/billing`).
    pub fn billing(&self) -> Billing {
        Billing::new(self.clone())
    }
    /// The Sessions API (agentic prompt loop).
    pub fn sessions(&self) -> Sessions {
        Sessions::new(self.clone())
    }
    /// The Memories API.
    pub fn memories(&self) -> Memories {
        Memories::new(self.clone())
    }
    /// The Plugins / marketplace API.
    pub fn plugins(&self) -> crate::resources::Plugins {
        crate::resources::Plugins::new(self.clone())
    }
    /// The project-management API (tasks/projects/todos/schedules/workflows).
    pub fn pm(&self) -> Pm {
        Pm::new(self.clone())
    }
    /// The feature-flags API.
    pub fn flags(&self) -> Flags {
        Flags::new(self.clone())
    }

    // ── Internal request plumbing ──────────────────────────────────────────

    /// The fully-qualified URL for a `/v1/...` path.
    pub(crate) fn url(&self, path: &str) -> String {
        let base = self.inner.base_url.trim_end_matches('/');
        if path.starts_with('/') {
            format!("{base}{path}")
        } else {
            format!("{base}/{path}")
        }
    }

    /// Resolve the auth token for the next request: the dynamic
    /// [`AuthProvider`](crate::AuthProvider) token when one is registered,
    /// otherwise the static API key.
    async fn auth_token(&self) -> Result<String> {
        match &self.inner.auth_provider {
            Some(provider) => provider.token().await,
            None => Ok(self.inner.api_key.clone()),
        }
    }

    /// Apply the standard headers (auth + version + content-type + user
    /// defaults) to a request builder. Async because resolving a dynamic auth
    /// token may require I/O.
    async fn apply_headers(
        &self,
        mut req: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder> {
        // Auth: send BOTH the Anthropic-style key header and the bearer. With a
        // dynamic provider this is a per-request short-lived token.
        let token = self.auth_token().await?;
        req = req
            .header("x-api-key", &token)
            .header("authorization", format!("Bearer {token}"))
            .header("anthropic-version", &self.inner.anthropic_version)
            .header("content-type", "application/json")
            .header("accept", "application/json");
        for (k, v) in &self.inner.default_headers {
            req = req.header(k.as_str(), v.as_str());
        }
        Ok(req)
    }

    /// Execute a JSON request with retries, deserializing the success body.
    pub(crate) async fn request_json<B, T>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T>
    where
        B: Serialize,
        T: DeserializeOwned,
    {
        let (bytes, _meta) = self.request_bytes(method, path, body).await?;
        serde_json::from_slice(&bytes).map_err(|e| Error::Decode(e.to_string()))
    }

    /// Execute a JSON request with retries, returning the raw success body bytes
    /// plus the response metadata.
    pub(crate) async fn request_bytes<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(Bytes, ResponseMeta)>
    where
        B: Serialize,
    {
        let url = self.url(path);
        // Pre-serialize the body once so retries reuse it.
        let payload: Option<Vec<u8>> = match body {
            Some(b) => {
                Some(serde_json::to_vec(b).map_err(|e| Error::Serialize(e.to_string()))?)
            }
            None => None,
        };

        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .apply_headers(self.inner.http.request(method.clone(), &url))
                .await?;
            if let Some(p) = &payload {
                req = req.body(p.clone());
            }

            let result = req.send().await;
            match result {
                Ok(resp) => {
                    let status = resp.status();
                    let meta = ResponseMeta::from_headers(resp.headers());
                    if let Some(hook) = &self.inner.response_hook {
                        hook(&meta);
                    }
                    if status.is_success() {
                        let bytes = resp.bytes().await?;
                        return Ok((bytes, meta));
                    }
                    // Error path: maybe retry.
                    let retryable = is_retryable_status(status.as_u16());
                    if retryable && attempt < self.inner.max_retries {
                        let delay = self.backoff(attempt, meta.retry_after);
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    let body_bytes = resp.bytes().await.unwrap_or_default();
                    return Err(Error::from_response_parts(
                        status.as_u16(),
                        meta.request_id,
                        &body_bytes,
                    ));
                }
                Err(e) => {
                    // Transport error: retry connection/timeout failures.
                    let transport_err: Error = e.into();
                    if attempt < self.inner.max_retries && transport_err.is_retryable() {
                        let delay = self.backoff(attempt, None);
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(transport_err);
                }
            }
        }
    }

    /// Open a streaming (SSE) request. Returns a byte stream of the response body
    /// plus the response metadata. Streaming requests do not auto-retry once the
    /// connection is established (a partial stream cannot be safely replayed);
    /// pre-connection transport failures DO retry.
    pub(crate) async fn request_stream<B>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<(
        impl Stream<Item = reqwest::Result<Bytes>>,
        ResponseMeta,
    )>
    where
        B: Serialize,
    {
        let url = self.url(path);
        let payload: Option<Vec<u8>> = match body {
            Some(b) => {
                Some(serde_json::to_vec(b).map_err(|e| Error::Serialize(e.to_string()))?)
            }
            None => None,
        };

        let mut attempt: u32 = 0;
        loop {
            let mut req = self
                .apply_headers(self.inner.http.request(method.clone(), &url))
                .await?
                .header("accept", "text/event-stream");
            if let Some(p) = &payload {
                req = req.body(p.clone());
            }

            match req.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    let meta = ResponseMeta::from_headers(resp.headers());
                    if let Some(hook) = &self.inner.response_hook {
                        hook(&meta);
                    }
                    if status.is_success() {
                        return Ok((resp.bytes_stream(), meta));
                    }
                    let retryable = is_retryable_status(status.as_u16());
                    if retryable && attempt < self.inner.max_retries {
                        let delay = self.backoff(attempt, meta.retry_after);
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    let body_bytes = resp.bytes().await.unwrap_or_default();
                    return Err(Error::from_response_parts(
                        status.as_u16(),
                        meta.request_id,
                        &body_bytes,
                    ));
                }
                Err(e) => {
                    let transport_err: Error = e.into();
                    if attempt < self.inner.max_retries && transport_err.is_retryable() {
                        let delay = self.backoff(attempt, None);
                        attempt += 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(transport_err);
                }
            }
        }
    }

    /// Compute the backoff delay for an attempt, honoring `retry-after`.
    fn backoff(&self, attempt: u32, retry_after: Option<u64>) -> Duration {
        if let Some(secs) = retry_after {
            // Cap an absurd server value so we never sleep forever.
            return Duration::from_secs(secs.min(60));
        }
        // Exponential backoff: 0.5s, 1s, 2s, ... capped at 8s.
        let base = Duration::from_millis(500);
        let factor = 1u32 << attempt.min(4);
        (base * factor).min(Duration::from_secs(8))
    }
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("base_url", &self.inner.base_url)
            .field("max_retries", &self.inner.max_retries)
            .field("api_key", &"***")
            .finish()
    }
}

/// Builder for a [`Client`].
pub struct ClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    anthropic_version: String,
    timeout: Duration,
    max_retries: u32,
    default_headers: Vec<(String, String)>,
    response_hook: Option<ResponseHook>,
    http: Option<reqwest::Client>,
    auth_provider: Option<SharedAuthProvider>,
}

impl Default for ClientBuilder {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: None,
            anthropic_version: ANTHROPIC_VERSION.to_string(),
            timeout: DEFAULT_TIMEOUT,
            max_retries: DEFAULT_MAX_RETRIES,
            default_headers: Vec::new(),
            response_hook: None,
            http: None,
            auth_provider: None,
        }
    }
}

impl ClientBuilder {
    /// A fresh builder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the API key explicitly (overrides the environment).
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }

    /// Override the base URL (default `https://api.simse.dev`, env
    /// `SIMSE_BASE_URL`).
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Override the `anthropic-version` header.
    pub fn anthropic_version(mut self, v: impl Into<String>) -> Self {
        self.anthropic_version = v.into();
        self
    }

    /// Set the per-request timeout.
    pub fn timeout(mut self, t: Duration) -> Self {
        self.timeout = t;
        self
    }

    /// Set the maximum number of retries.
    pub fn max_retries(mut self, n: u32) -> Self {
        self.max_retries = n;
        self
    }

    /// Add a default header sent on every request.
    pub fn default_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.default_headers.push((name.into(), value.into()));
        self
    }

    /// Register a hook invoked with the [`ResponseMeta`] of every response.
    pub fn on_response(mut self, hook: ResponseHook) -> Self {
        self.response_hook = Some(hook);
        self
    }

    /// Supply a pre-built `reqwest::Client` (advanced — proxies, custom TLS).
    pub fn http_client(mut self, client: reqwest::Client) -> Self {
        self.http = Some(client);
        self
    }

    /// Register a dynamic [`AuthProvider`](crate::AuthProvider). When set, a
    /// fresh token is fetched before every request (the
    /// [`token`](crate::AuthProvider::token) result is sent as BOTH the
    /// `x-api-key` and the `Authorization: Bearer` value), instead of the static
    /// API key. A provider removes the requirement to supply an API key.
    pub fn auth_provider(mut self, provider: SharedAuthProvider) -> Self {
        self.auth_provider = Some(provider);
        self
    }

    /// Resolve the API key from the builder or the environment. When a dynamic
    /// [`AuthProvider`](crate::AuthProvider) is registered, the static key is
    /// optional — the provider supplies the per-request token — so an absent key
    /// resolves to an empty string rather than an error.
    fn resolve_api_key(&self) -> Result<String> {
        if let Some(k) = &self.api_key {
            if k.is_empty() {
                return Err(Error::Config("api_key must not be empty".into()));
            }
            return Ok(k.clone());
        }
        match std::env::var("SIMSE_API_KEY").or_else(|_| std::env::var("ANTHROPIC_API_KEY")) {
            Ok(k) => Ok(k),
            Err(_) if self.auth_provider.is_some() => Ok(String::new()),
            Err(_) => Err(Error::Config(
                "no API key: pass one to ClientBuilder::api_key, register an \
                 auth_provider, or set SIMSE_API_KEY / ANTHROPIC_API_KEY"
                    .into(),
            )),
        }
    }

    /// Build the [`Client`].
    pub fn build(self) -> Result<Client> {
        let api_key = self.resolve_api_key()?;
        let base_url = self
            .base_url
            .clone()
            .or_else(|| std::env::var("SIMSE_BASE_URL").ok())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        let http = match self.http {
            Some(c) => c,
            None => reqwest::Client::builder()
                .timeout(self.timeout)
                .user_agent(concat!("simse-rust/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| Error::Config(format!("failed to build HTTP client: {e}")))?,
        };

        Ok(Client {
            inner: Arc::new(Inner {
                http,
                base_url,
                api_key,
                anthropic_version: self.anthropic_version,
                max_retries: self.max_retries,
                default_headers: self.default_headers,
                response_hook: self.response_hook,
                auth_provider: self.auth_provider,
            }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_url() {
        let c = Client::new("sk_test").unwrap();
        assert_eq!(c.url("/v1/messages"), "https://api.simse.dev/v1/messages");
        assert_eq!(c.url("v1/models"), "https://api.simse.dev/v1/models");
    }

    #[test]
    fn base_url_override_trims_slash() {
        let c = Client::builder()
            .api_key("sk_x")
            .base_url("http://localhost:6080/")
            .build()
            .unwrap();
        assert_eq!(c.url("/v1/account"), "http://localhost:6080/v1/account");
    }

    #[test]
    fn empty_key_is_config_error() {
        let err = Client::new("").unwrap_err();
        assert!(matches!(err, Error::Config(_)));
    }

    #[test]
    fn backoff_honors_retry_after() {
        let c = Client::new("sk_x").unwrap();
        assert_eq!(c.backoff(0, Some(3)), Duration::from_secs(3));
        // Caps an absurd value.
        assert_eq!(c.backoff(0, Some(9999)), Duration::from_secs(60));
        // Exponential when no retry-after.
        assert_eq!(c.backoff(0, None), Duration::from_millis(500));
        assert_eq!(c.backoff(1, None), Duration::from_secs(1));
        assert_eq!(c.backoff(2, None), Duration::from_secs(2));
    }
}
