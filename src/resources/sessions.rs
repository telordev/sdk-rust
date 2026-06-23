//! The Sessions API (`/v1/sessions*`) — the agentic prompt loop.

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};

use crate::client::Client;
use crate::error::{Error, Result};
use crate::sse::SseDecoder;
use crate::types::{
    ResumedSession, Session, SessionList, SessionPromptResult, SessionStreamEvent,
};

/// The sessions resource.
#[derive(Clone)]
pub struct Sessions {
    client: Client,
}

/// Parameters for creating a session.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionCreateParams {
    /// Pin a model for the session.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// An optional title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Per-session system prompt (persona + guardrails). Injected on every
    /// prompt turn and resume (spec §2.2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<String>,
    /// Arbitrary key-value metadata attached to the session (e.g. team, role).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<std::collections::HashMap<String, String>>,
}

impl SessionCreateParams {
    /// Empty params.
    pub fn new() -> Self {
        Self::default()
    }
    /// Set the model.
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }
    /// Set the title.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }
    /// Set a per-session system prompt.
    pub fn system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }
    /// Attach arbitrary key-value metadata to the session.
    pub fn metadata(mut self, metadata: std::collections::HashMap<String, String>) -> Self {
        self.metadata = Some(metadata);
        self
    }
}

impl Sessions {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `POST /v1/sessions` → create a session.
    pub async fn create(&self, params: SessionCreateParams) -> Result<Session> {
        self.client
            .request_json(Method::POST, "/v1/sessions", Some(&params))
            .await
    }

    /// `GET /v1/sessions` → list the caller's sessions.
    pub async fn list(&self) -> Result<SessionList> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/sessions", None)
            .await
    }

    /// `GET /v1/sessions/{id}` → one session summary.
    pub async fn retrieve(&self, id: impl AsRef<str>) -> Result<crate::types::SessionSummary> {
        let path = format!("/v1/sessions/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `DELETE /v1/sessions/{id}` → delete a session.
    pub async fn delete(&self, id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/sessions/{}", id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }

    /// `POST /v1/sessions/{id}/messages` (`stream:false`) → the buffered
    /// assistant turn.
    pub async fn prompt(
        &self,
        id: impl AsRef<str>,
        content: impl Into<String>,
    ) -> Result<SessionPromptResult> {
        let path = format!("/v1/sessions/{}/messages", id.as_ref());
        let body = json!({ "content": content.into(), "stream": false });
        self.client.request_json(Method::POST, &path, Some(&body)).await
    }

    /// `POST /v1/sessions/{id}/messages` (`stream:true`) → a stream of typed
    /// agentic events ([`SessionStreamEvent`]).
    pub async fn stream(
        &self,
        id: impl AsRef<str>,
        content: impl Into<String>,
    ) -> Result<SessionPromptStream> {
        let path = format!("/v1/sessions/{}/messages", id.as_ref());
        let body = json!({ "content": content.into(), "stream": true });
        let (byte_stream, _meta) = self
            .client
            .request_stream(Method::POST, &path, Some(&body))
            .await?;
        Ok(SessionPromptStream::new(byte_stream))
    }

    /// `POST /v1/sessions/{id}/resume` → reconstruct history.
    pub async fn resume(&self, id: impl AsRef<str>) -> Result<ResumedSession> {
        let path = format!("/v1/sessions/{}/resume", id.as_ref());
        self.client
            .request_json::<(), _>(Method::POST, &path, None)
            .await
    }

    /// `POST /v1/sessions/{id}/abort` → cancel an in-flight prompt.
    pub async fn abort(&self, id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/sessions/{}/abort", id.as_ref());
        self.client.request_json::<(), _>(Method::POST, &path, None).await
    }
}

/// A streaming session-prompt response: a `Stream` of [`SessionStreamEvent`].
///
/// The session path uses anonymous `data: {json}` lines terminated by a
/// `data: [DONE]` sentinel (not the named-event Messages framing).
pub struct SessionPromptStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    decoder: SseDecoder,
    pending: VecDeque<Result<SessionStreamEvent>>,
    done: bool,
}

impl SessionPromptStream {
    pub(crate) fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            decoder: SseDecoder::new(),
            pending: VecDeque::new(),
            done: false,
        }
    }

    /// Drive the stream and collect the final assistant text (concatenating
    /// `delta` fragments, preferring the authoritative `Done.text`).
    pub async fn collect_text(mut self) -> Result<String> {
        let mut acc = String::new();
        while let Some(ev) = self.next().await {
            match ev? {
                SessionStreamEvent::Delta { delta } => acc.push_str(&delta),
                SessionStreamEvent::Done { text, .. } => {
                    if !text.is_empty() {
                        acc = text;
                    }
                    break;
                }
                SessionStreamEvent::Error { message } => {
                    return Err(Error::Stream(message));
                }
                _ => {}
            }
        }
        Ok(acc)
    }

    fn parse_chunk(&mut self, events: Vec<crate::sse::SseEvent>) {
        for ev in events {
            let data = ev.data.trim();
            if data == "[DONE]" {
                self.done = true;
                continue;
            }
            if data.is_empty() {
                continue;
            }
            match serde_json::from_str::<SessionStreamEvent>(data) {
                Ok(parsed) => self.pending.push_back(Ok(parsed)),
                Err(e) => self
                    .pending
                    .push_back(Err(Error::Decode(format!("bad session event: {e}")))),
            }
        }
    }
}

impl Stream for SessionPromptStream {
    type Item = Result<SessionStreamEvent>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            if let Some(item) = this.pending.pop_front() {
                return Poll::Ready(Some(item));
            }
            if this.done {
                return Poll::Ready(None);
            }
            match this.inner.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(chunk))) => {
                    let events = this.decoder.push(&chunk);
                    this.parse_chunk(events);
                }
                Poll::Ready(Some(Err(e))) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(Error::Connection(e.to_string()))));
                }
                Poll::Ready(None) => {
                    if let Some(ev) = this.decoder.finish() {
                        this.parse_chunk(vec![ev]);
                    }
                    this.done = true;
                    if let Some(item) = this.pending.pop_front() {
                        return Poll::Ready(Some(item));
                    }
                    return Poll::Ready(None);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}
