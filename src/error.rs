//! Typed error hierarchy mirroring the Anthropic SDK.
//!
//! [`Error`] is the single `Result` error type. The API-side variants
//! ([`Error::Api`]) carry an [`ApiErrorBody`] with the HTTP `status`, the
//! `request-id`, and the Anthropic `error.type` discriminator. Convenience
//! constructors map each documented status to its named class, and
//! [`Error::from_response_parts`] parses **both** wire envelopes the gateway
//! emits:
//!
//! - the Anthropic envelope `{"type":"error","error":{"type","message"},"request_id"}`
//!   (Messages / Models surfaces), and
//! - the legacy envelope `{"error":{"code","message"}}` (platform / dashboard
//!   surfaces).

use std::fmt;

use serde_json::Value;

/// The kind of an API error, derived from the HTTP status (and cross-checked
/// against the wire `error.type` when present). Mirrors the Anthropic SDK error
/// classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApiErrorKind {
    /// `400 invalid_request_error`
    BadRequest,
    /// `401 authentication_error`
    Authentication,
    /// `403 permission_error`
    PermissionDenied,
    /// `404 not_found_error`
    NotFound,
    /// `409 conflict`
    Conflict,
    /// `413 request_too_large`
    RequestTooLarge,
    /// `422 unprocessable entity`
    UnprocessableEntity,
    /// `429 rate_limit_error`
    RateLimit,
    /// `500 api_error`
    InternalServer,
    /// `503 overloaded_error`
    Overloaded,
    /// Any other 4xx/5xx the gateway returns.
    Other,
}

impl ApiErrorKind {
    /// Classify an HTTP status code into an [`ApiErrorKind`].
    pub fn from_status(status: u16) -> Self {
        match status {
            400 => Self::BadRequest,
            401 => Self::Authentication,
            403 => Self::PermissionDenied,
            404 => Self::NotFound,
            409 => Self::Conflict,
            413 => Self::RequestTooLarge,
            422 => Self::UnprocessableEntity,
            429 => Self::RateLimit,
            500 => Self::InternalServer,
            503 => Self::Overloaded,
            _ => Self::Other,
        }
    }

    /// The canonical Anthropic `error.type` discriminator for this kind.
    pub fn type_str(self) -> &'static str {
        match self {
            Self::BadRequest => "invalid_request_error",
            Self::Authentication => "authentication_error",
            Self::PermissionDenied => "permission_error",
            Self::NotFound => "not_found_error",
            Self::Conflict => "conflict",
            Self::RequestTooLarge => "request_too_large",
            Self::UnprocessableEntity => "unprocessable_entity",
            Self::RateLimit => "rate_limit_error",
            Self::InternalServer => "api_error",
            Self::Overloaded => "overloaded_error",
            Self::Other => "api_error",
        }
    }

    /// Human-readable name for the class (matches the Anthropic SDK names).
    pub fn class_name(self) -> &'static str {
        match self {
            Self::BadRequest => "BadRequestError",
            Self::Authentication => "AuthenticationError",
            Self::PermissionDenied => "PermissionDeniedError",
            Self::NotFound => "NotFoundError",
            Self::Conflict => "ConflictError",
            Self::RequestTooLarge => "RequestTooLargeError",
            Self::UnprocessableEntity => "UnprocessableEntityError",
            Self::RateLimit => "RateLimitError",
            Self::InternalServer => "InternalServerError",
            Self::Overloaded => "OverloadedError",
            Self::Other => "APIStatusError",
        }
    }
}

/// The structured body of an API error — the `APIError` base in the Anthropic
/// SDKs (`.status`, `.request_id`, `.type`).
#[derive(Debug, Clone)]
pub struct ApiErrorBody {
    /// The HTTP status code.
    pub status: u16,
    /// Classified error kind (named subclass).
    pub kind: ApiErrorKind,
    /// The wire `error.type` (Anthropic envelope) or `error.code` (legacy
    /// envelope). Falls back to [`ApiErrorKind::type_str`] when neither is
    /// present.
    pub error_type: String,
    /// Human-readable message extracted from the body.
    pub message: String,
    /// The `request-id` response header (and/or `request_id` body field).
    pub request_id: Option<String>,
    /// The raw JSON body, when it parsed.
    pub raw: Option<Value>,
}

impl ApiErrorBody {
    /// `true` when this error is retryable per the contract (408/409/429/≥500).
    pub fn is_retryable(&self) -> bool {
        is_retryable_status(self.status)
    }
}

impl fmt::Display for ApiErrorBody {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} (status {}, type {}): {}",
            self.kind.class_name(),
            self.status,
            self.error_type,
            self.message
        )?;
        if let Some(rid) = &self.request_id {
            write!(f, " [request-id: {rid}]")?;
        }
        Ok(())
    }
}

/// Whether an HTTP status is retryable per the contract: 408, 409, 429, ≥500.
pub fn is_retryable_status(status: u16) -> bool {
    matches!(status, 408 | 409 | 429) || status >= 500
}

/// The crate's unified error type.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A typed API error returned by the gateway (4xx/5xx with a parsed body).
    #[error("{0}")]
    Api(ApiErrorBody),

    /// The request could not reach the API (DNS, connect, TLS, body I/O). The
    /// Anthropic `APIConnectionError`.
    #[error("connection error: {0}")]
    Connection(String),

    /// The request timed out. The Anthropic `APITimeoutError`.
    #[error("request timed out after {0:?}")]
    Timeout(std::time::Duration),

    /// A response body could not be deserialized into the expected type.
    #[error("failed to decode response: {0}")]
    Decode(String),

    /// A streaming SSE frame was malformed.
    #[error("stream error: {0}")]
    Stream(String),

    /// The client was misconfigured (e.g. missing API key, bad base URL).
    #[error("configuration error: {0}")]
    Config(String),

    /// A request body could not be serialized.
    #[error("failed to serialize request: {0}")]
    Serialize(String),
}

impl Error {
    /// Build the typed API error from the HTTP status, the `request-id` header,
    /// and the (possibly empty) response body bytes. Parses both wire envelopes.
    pub fn from_response_parts(status: u16, request_id: Option<String>, body: &[u8]) -> Self {
        let raw: Option<Value> = serde_json::from_slice(body).ok();
        let kind = ApiErrorKind::from_status(status);

        // Extract `error.type`/`error.code` + `message`, plus a `request_id`
        // body field (the Anthropic envelope carries it in-body too).
        let (error_type, message, body_request_id) = match &raw {
            Some(v) => {
                let err = v.get("error");
                let etype = err
                    .and_then(|e| e.get("type"))
                    .or_else(|| err.and_then(|e| e.get("code")))
                    .and_then(|t| t.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| kind.type_str().to_string());
                let msg = err
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
                    .or_else(|| v.get("message").and_then(|m| m.as_str()).map(str::to_string))
                    .unwrap_or_else(|| default_message(kind));
                let body_rid = v
                    .get("request_id")
                    .and_then(|r| r.as_str())
                    .map(str::to_string);
                (etype, msg, body_rid)
            }
            None => (
                kind.type_str().to_string(),
                if body.is_empty() {
                    default_message(kind)
                } else {
                    String::from_utf8_lossy(body).trim().to_string()
                },
                None,
            ),
        };

        Error::Api(ApiErrorBody {
            status,
            kind,
            error_type,
            message,
            request_id: request_id.or(body_request_id),
            raw,
        })
    }

    /// The HTTP status, if this is an API error.
    pub fn status(&self) -> Option<u16> {
        match self {
            Error::Api(b) => Some(b.status),
            _ => None,
        }
    }

    /// The `request-id`, if available.
    pub fn request_id(&self) -> Option<&str> {
        match self {
            Error::Api(b) => b.request_id.as_deref(),
            _ => None,
        }
    }

    /// The Anthropic `error.type`, if this is an API error.
    pub fn error_type(&self) -> Option<&str> {
        match self {
            Error::Api(b) => Some(&b.error_type),
            _ => None,
        }
    }

    /// `true` when retrying this error may succeed (retryable status, a
    /// connection error, or a timeout).
    pub fn is_retryable(&self) -> bool {
        match self {
            Error::Api(b) => b.is_retryable(),
            Error::Connection(_) | Error::Timeout(_) => true,
            _ => false,
        }
    }

    // ── Named-class predicates (mirror Anthropic SDK subclasses) ────────────

    /// `400 BadRequestError`.
    pub fn is_bad_request(&self) -> bool {
        self.is_kind(ApiErrorKind::BadRequest)
    }
    /// `401 AuthenticationError`.
    pub fn is_authentication(&self) -> bool {
        self.is_kind(ApiErrorKind::Authentication)
    }
    /// `403 PermissionDeniedError`.
    pub fn is_permission_denied(&self) -> bool {
        self.is_kind(ApiErrorKind::PermissionDenied)
    }
    /// `404 NotFoundError`.
    pub fn is_not_found(&self) -> bool {
        self.is_kind(ApiErrorKind::NotFound)
    }
    /// `409 ConflictError`.
    pub fn is_conflict(&self) -> bool {
        self.is_kind(ApiErrorKind::Conflict)
    }
    /// `413 RequestTooLargeError`.
    pub fn is_request_too_large(&self) -> bool {
        self.is_kind(ApiErrorKind::RequestTooLarge)
    }
    /// `422 UnprocessableEntityError`.
    pub fn is_unprocessable_entity(&self) -> bool {
        self.is_kind(ApiErrorKind::UnprocessableEntity)
    }
    /// `429 RateLimitError`.
    pub fn is_rate_limit(&self) -> bool {
        self.is_kind(ApiErrorKind::RateLimit)
    }
    /// `500 InternalServerError`.
    pub fn is_internal_server(&self) -> bool {
        self.is_kind(ApiErrorKind::InternalServer)
    }
    /// `503 OverloadedError`.
    pub fn is_overloaded(&self) -> bool {
        self.is_kind(ApiErrorKind::Overloaded)
    }

    fn is_kind(&self, kind: ApiErrorKind) -> bool {
        matches!(self, Error::Api(b) if b.kind == kind)
    }
}

/// A default message for a status when the body carried none.
fn default_message(kind: ApiErrorKind) -> String {
    match kind {
        ApiErrorKind::BadRequest => "invalid request",
        ApiErrorKind::Authentication => "authentication failed",
        ApiErrorKind::PermissionDenied => "permission denied",
        ApiErrorKind::NotFound => "not found",
        ApiErrorKind::Conflict => "conflict",
        ApiErrorKind::RequestTooLarge => "request too large",
        ApiErrorKind::UnprocessableEntity => "unprocessable entity",
        ApiErrorKind::RateLimit => "rate limited",
        ApiErrorKind::InternalServer => "internal server error",
        ApiErrorKind::Overloaded => "overloaded",
        ApiErrorKind::Other => "request failed",
    }
    .to_string()
}

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            // reqwest does not expose the configured duration here; surface a
            // zero placeholder — the message carries the detail.
            Error::Timeout(std::time::Duration::ZERO)
        } else if e.is_decode() {
            Error::Decode(e.to_string())
        } else {
            Error::Connection(e.to_string())
        }
    }
}

/// The crate `Result` alias.
pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_statuses() {
        assert_eq!(ApiErrorKind::from_status(400), ApiErrorKind::BadRequest);
        assert_eq!(ApiErrorKind::from_status(401), ApiErrorKind::Authentication);
        assert_eq!(ApiErrorKind::from_status(403), ApiErrorKind::PermissionDenied);
        assert_eq!(ApiErrorKind::from_status(404), ApiErrorKind::NotFound);
        assert_eq!(ApiErrorKind::from_status(409), ApiErrorKind::Conflict);
        assert_eq!(ApiErrorKind::from_status(413), ApiErrorKind::RequestTooLarge);
        assert_eq!(
            ApiErrorKind::from_status(422),
            ApiErrorKind::UnprocessableEntity
        );
        assert_eq!(ApiErrorKind::from_status(429), ApiErrorKind::RateLimit);
        assert_eq!(ApiErrorKind::from_status(500), ApiErrorKind::InternalServer);
        assert_eq!(ApiErrorKind::from_status(503), ApiErrorKind::Overloaded);
        assert_eq!(ApiErrorKind::from_status(418), ApiErrorKind::Other);
    }

    #[test]
    fn parses_anthropic_envelope() {
        let body = br#"{"type":"error","error":{"type":"invalid_request_error","message":"bad model"},"request_id":"req_abc"}"#;
        let err = Error::from_response_parts(400, None, body);
        assert!(err.is_bad_request());
        assert_eq!(err.error_type(), Some("invalid_request_error"));
        assert_eq!(err.request_id(), Some("req_abc"));
        match err {
            Error::Api(b) => assert_eq!(b.message, "bad model"),
            _ => panic!("expected Api"),
        }
    }

    #[test]
    fn parses_legacy_envelope() {
        let body = br#"{"error":{"code":"unauthenticated","message":"bad token"}}"#;
        let err = Error::from_response_parts(401, Some("req_h".into()), body);
        assert!(err.is_authentication());
        // The legacy `code` is surfaced as the error_type.
        assert_eq!(err.error_type(), Some("unauthenticated"));
        // Header request-id wins when the body has none.
        assert_eq!(err.request_id(), Some("req_h"));
    }

    #[test]
    fn retryable_classification() {
        assert!(is_retryable_status(429));
        assert!(is_retryable_status(500));
        assert!(is_retryable_status(503));
        assert!(is_retryable_status(408));
        assert!(is_retryable_status(409));
        assert!(!is_retryable_status(400));
        assert!(!is_retryable_status(404));
    }

    #[test]
    fn classifies_conflict_and_unprocessable() {
        let conflict = Error::from_response_parts(409, None, b"");
        assert!(conflict.is_conflict());
        assert!(!conflict.is_not_found());
        // 409 stays retryable per the contract.
        assert!(conflict.is_retryable());
        match conflict {
            Error::Api(b) => assert_eq!(b.kind.class_name(), "ConflictError"),
            _ => panic!("expected Api"),
        }

        let unproc = Error::from_response_parts(422, None, b"");
        assert!(unproc.is_unprocessable_entity());
        // 422 is not in the retryable set.
        assert!(!unproc.is_retryable());
        match unproc {
            Error::Api(b) => assert_eq!(b.kind.class_name(), "UnprocessableEntityError"),
            _ => panic!("expected Api"),
        }
    }

    #[test]
    fn empty_body_gets_default_message() {
        let err = Error::from_response_parts(429, None, b"");
        match err {
            Error::Api(b) => assert_eq!(b.message, "rate limited"),
            _ => panic!(),
        }
    }
}
