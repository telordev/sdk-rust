//! Dynamic authentication: a pluggable [`AuthProvider`] that yields a
//! short-lived token refreshed on every request.
//!
//! The default client auth is a static API key (sent as both `x-api-key` and
//! `Authorization: Bearer`). For callers that mint short-lived credentials —
//! the Simse CLI exchanges its session for a rotating token — register an
//! [`AuthProvider`] via [`ClientBuilder::auth_provider`]. When set, the
//! provider's [`token`](AuthProvider::token) is invoked per request and its
//! result is sent as BOTH the `x-api-key` and the bearer value.
//!
//! [`ClientBuilder::auth_provider`]: crate::ClientBuilder::auth_provider

use std::sync::Arc;

use crate::error::Result;

/// Supplies a fresh auth token on each request.
///
/// Implementations are expected to be cheap to call repeatedly (cache + refresh
/// internally if minting a token is expensive). The returned `String` is used
/// verbatim as both the `x-api-key` header and the `Authorization: Bearer`
/// value.
///
/// # Example
///
/// ```no_run
/// use std::sync::Arc;
/// use simse::{AuthProvider, Client, Result};
///
/// struct CliTokenProvider;
///
/// #[async_trait::async_trait]
/// impl AuthProvider for CliTokenProvider {
///     async fn token(&self) -> Result<String> {
///         // Exchange the local session for a short-lived token here.
///         Ok("st_short_lived_token".to_string())
///     }
/// }
///
/// # fn build() -> Result<Client> {
/// let client = Client::builder()
///     .auth_provider(Arc::new(CliTokenProvider))
///     .build()?;
/// # Ok(client)
/// # }
/// ```
#[async_trait::async_trait]
pub trait AuthProvider: Send + Sync {
    /// Produce the token to authenticate the next request.
    async fn token(&self) -> Result<String>;
}

/// A shared, dynamically-dispatched [`AuthProvider`] handle.
pub type SharedAuthProvider = Arc<dyn AuthProvider>;
