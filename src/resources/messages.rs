//! The Messages API (`/v1/messages`, `/v1/messages/count_tokens`).

use std::pin::Pin;
use std::task::{Context, Poll};

use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::Method;

use crate::client::{Client, ResponseMeta};
use crate::error::{Error, Result};
use crate::sse::{MessageAccumulator, SseDecoder};
use crate::types::{ContentDelta, Message, MessageCreateParams, MessageStreamEvent, TokenCount};

/// The Messages resource.
#[derive(Clone)]
pub struct Messages {
    client: Client,
}

impl Messages {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `POST /v1/messages` (non-streaming) → a final [`Message`].
    pub async fn create(&self, params: MessageCreateParams) -> Result<Message> {
        let mut params = params;
        params.stream = Some(false);
        self.client
            .request_json(Method::POST, "/v1/messages", Some(&params))
            .await
    }

    /// `POST /v1/messages` (non-streaming) → the [`Message`] plus the response
    /// metadata (request-id + rate-limit headers).
    pub async fn create_with_meta(
        &self,
        params: MessageCreateParams,
    ) -> Result<(Message, ResponseMeta)> {
        let mut params = params;
        params.stream = Some(false);
        let (bytes, meta) = self
            .client
            .request_bytes(Method::POST, "/v1/messages", Some(&params))
            .await?;
        let msg = serde_json::from_slice(&bytes).map_err(|e| Error::Decode(e.to_string()))?;
        Ok((msg, meta))
    }

    /// `POST /v1/messages` with `stream:true` → a [`MessageStream`] of typed
    /// events that also accumulates a final [`Message`].
    pub async fn stream(&self, params: MessageCreateParams) -> Result<MessageStream> {
        let mut params = params;
        params.stream = Some(true);
        let (byte_stream, _meta) = self
            .client
            .request_stream(Method::POST, "/v1/messages", Some(&params))
            .await?;
        Ok(MessageStream::new(byte_stream))
    }

    /// `POST /v1/messages/count_tokens` → the estimated input-token count.
    pub async fn count_tokens(&self, params: MessageCreateParams) -> Result<TokenCount> {
        // The endpoint ignores `max_tokens`/`stream`; send the body as-is but
        // clear `stream` for hygiene.
        let mut params = params;
        params.stream = None;
        self.client
            .request_json(Method::POST, "/v1/messages/count_tokens", Some(&params))
            .await
    }
}

/// A streaming Messages response: a `Stream` of [`MessageStreamEvent`] that also
/// folds events into a final [`Message`] (`final_message()` /
/// `accumulate().await`).
pub struct MessageStream {
    inner: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    decoder: SseDecoder,
    /// Decoded-but-not-yet-yielded events (one chunk can hold several frames).
    pending: std::collections::VecDeque<Result<MessageStreamEvent>>,
    accumulator: MessageAccumulator,
    done: bool,
}

impl MessageStream {
    pub(crate) fn new<S>(stream: S) -> Self
    where
        S: Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
    {
        Self {
            inner: Box::pin(stream),
            decoder: SseDecoder::new(),
            pending: std::collections::VecDeque::new(),
            accumulator: MessageAccumulator::new(),
            done: false,
        }
    }

    /// Drive the stream to completion, applying every event, and return the
    /// accumulated final [`Message`]. Errors if a stream error frame is seen or
    /// no `message_start` arrived.
    pub async fn accumulate(mut self) -> Result<Message> {
        while let Some(ev) = self.next().await {
            // `next()` already applied the event to the accumulator; just
            // propagate the first error.
            ev?;
        }
        self.accumulator.into_message()
    }

    /// Borrow the in-progress message (after at least `message_start`).
    pub fn current_message(&self) -> Option<&Message> {
        self.accumulator.current()
    }

    /// Drive the stream to completion and return only the accumulated assistant
    /// text (the concatenation of every `text` block). Mirrors the TS
    /// `finalText()` / Python `get_final_text()` convenience.
    pub async fn final_text(self) -> Result<String> {
        Ok(self.accumulate().await?.text())
    }

    /// Adapt this event stream into a `Stream` of just the text deltas — each
    /// item is one `text_delta` fragment (errors propagate). Mirrors the TS
    /// `textStream` / Python `text_stream` convenience. Other event kinds
    /// (tool-input deltas, pings, lifecycle events) are filtered out; the
    /// accumulator is still driven, so `tool_use` blocks remain reconstructable
    /// off the underlying message if needed.
    pub fn text_stream(self) -> impl Stream<Item = Result<String>> + Send {
        self.filter_map(|ev| async move {
            match ev {
                Ok(MessageStreamEvent::ContentBlockDelta {
                    delta: ContentDelta::TextDelta { text },
                    ..
                }) => Some(Ok(text)),
                Ok(_) => None,
                Err(e) => Some(Err(e)),
            }
        })
    }

    /// Refill `pending` from the byte stream. Returns `false` when the stream is
    /// exhausted.
    fn parse_into_pending(&mut self, raw: &SseEvents) {
        for ev in &raw.0 {
            // The Messages API uses named events. The session path uses
            // anonymous `data: {json}` with a terminal `[DONE]` — but this is
            // the Messages stream, so a bare `[DONE]` (defensive) ends it.
            if ev.data.trim() == "[DONE]" {
                self.done = true;
                continue;
            }
            if ev.data.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<MessageStreamEvent>(&ev.data) {
                Ok(parsed) => {
                    // Apply to the accumulator before yielding, so `accumulate`
                    // and the public stream share one fold.
                    if let Err(e) = self.accumulator.apply(&parsed) {
                        self.pending.push_back(Err(e));
                        continue;
                    }
                    if let MessageStreamEvent::Error { error } = &parsed {
                        let msg = if error.message.is_empty() {
                            "stream error".to_string()
                        } else {
                            error.message.clone()
                        };
                        self.pending.push_back(Ok(parsed));
                        self.pending.push_back(Err(Error::Stream(msg)));
                        continue;
                    }
                    self.pending.push_back(Ok(parsed));
                }
                Err(e) => self
                    .pending
                    .push_back(Err(Error::Decode(format!("bad stream event: {e}")))),
            }
        }
    }
}

/// Small newtype so `parse_into_pending` borrows cleanly.
struct SseEvents(Vec<crate::sse::SseEvent>);

impl Stream for MessageStream {
    type Item = Result<MessageStreamEvent>;

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
                    let events = SseEvents(this.decoder.push(&chunk));
                    this.parse_into_pending(&events);
                    // loop to drain pending
                }
                Poll::Ready(Some(Err(e))) => {
                    this.done = true;
                    return Poll::Ready(Some(Err(Error::Connection(e.to_string()))));
                }
                Poll::Ready(None) => {
                    // Flush any trailing frame.
                    if let Some(ev) = this.decoder.finish() {
                        let events = SseEvents(vec![ev]);
                        this.parse_into_pending(&events);
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

