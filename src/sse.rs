//! A minimal, allocation-light Server-Sent Events parser and the streaming
//! accumulator that folds Messages-API events into a final [`Message`].
//!
//! The parser is a pure state machine over byte chunks: feed it bytes with
//! [`SseDecoder::push`] and drain complete events. It handles the two framings
//! the gateway emits:
//!   - **named events** (`event: <type>\ndata: <json>\n\n`) — Messages / Models,
//!   - **anonymous data lines** (`data: <json>\n\n`, terminal `data: [DONE]`) —
//!     the session prompt path.

use crate::error::{Error, Result};
use crate::types::{ContentBlock, ContentDelta, Message, MessageStreamEvent};

/// A raw SSE event: the optional `event:` name and the joined `data:` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SseEvent {
    /// The `event:` field (empty when the frame had none).
    pub event: String,
    /// The joined `data:` lines (newline-separated, per the SSE spec).
    pub data: String,
}

/// An incremental SSE byte decoder. Push bytes; pull complete events.
#[derive(Debug, Default)]
pub struct SseDecoder {
    /// Bytes received but not yet forming a complete frame.
    buf: String,
}

impl SseDecoder {
    /// A fresh decoder.
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed a chunk of bytes; returns every complete event the buffer now holds.
    ///
    /// Invalid UTF-8 is lossily decoded (SSE payloads are JSON text).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        self.drain()
    }

    /// Drain all complete events (frames terminated by a blank line).
    fn drain(&mut self) -> Vec<SseEvent> {
        let mut out = Vec::new();
        // Normalize CRLF to LF so the `\n\n` frame delimiter works either way.
        if self.buf.contains('\r') {
            self.buf = self.buf.replace("\r\n", "\n").replace('\r', "\n");
        }
        while let Some(idx) = self.buf.find("\n\n") {
            let frame: String = self.buf.drain(..idx + 2).collect();
            if let Some(ev) = parse_frame(&frame) {
                out.push(ev);
            }
        }
        out
    }

    /// Flush a trailing frame that was not terminated by a blank line (e.g. the
    /// stream closed cleanly without a final `\n\n`).
    pub fn finish(&mut self) -> Option<SseEvent> {
        if self.buf.trim().is_empty() {
            self.buf.clear();
            return None;
        }
        let frame = std::mem::take(&mut self.buf);
        parse_frame(&frame)
    }
}

/// Parse a single frame (a block of `field: value` lines) into an [`SseEvent`].
/// Returns `None` for comment-only / empty frames.
fn parse_frame(frame: &str) -> Option<SseEvent> {
    let mut event = String::new();
    let mut data_lines: Vec<&str> = Vec::new();

    for line in frame.split('\n') {
        if line.is_empty() || line.starts_with(':') {
            // Blank line (frame terminator) or comment — skip.
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((f, v)) => (f, v.strip_prefix(' ').unwrap_or(v)),
            None => (line, ""),
        };
        match field {
            "event" => event = value.to_string(),
            "data" => data_lines.push(value),
            _ => {} // id / retry / unknown fields ignored
        }
    }

    if data_lines.is_empty() && event.is_empty() {
        return None;
    }
    Some(SseEvent {
        event,
        data: data_lines.join("\n"),
    })
}

/// Accumulates Messages-API [`MessageStreamEvent`]s into a final [`Message`],
/// folding `text_delta` and `input_json_delta` fragments into their blocks.
#[derive(Debug, Default)]
pub struct MessageAccumulator {
    message: Option<Message>,
    /// Per-index buffer of streamed tool-input JSON fragments.
    tool_json: Vec<String>,
}

impl MessageAccumulator {
    /// A fresh accumulator.
    pub fn new() -> Self {
        Self::default()
    }

    /// Apply one event, mutating the in-progress message.
    pub fn apply(&mut self, event: &MessageStreamEvent) -> Result<()> {
        match event {
            MessageStreamEvent::MessageStart { message } => {
                self.message = Some(message.clone());
                self.tool_json.clear();
            }
            MessageStreamEvent::ContentBlockStart {
                index,
                content_block,
            } => {
                let idx = *index;
                grow(&mut self.tool_json, idx, String::new());
                // Reset any tool-json buffer for a fresh tool_use block.
                if matches!(content_block, ContentBlock::ToolUse { .. }) {
                    self.tool_json[idx] = String::new();
                }
                let msg = self.message_mut()?;
                grow(&mut msg.content, idx, ContentBlock::Text { text: String::new() });
                msg.content[idx] = content_block.clone();
            }
            MessageStreamEvent::ContentBlockDelta { index, delta } => {
                let idx = *index;
                {
                    let msg = self.message_mut()?;
                    grow(&mut msg.content, idx, ContentBlock::Text { text: String::new() });
                }
                grow(&mut self.tool_json, idx, String::new());
                match delta {
                    ContentDelta::TextDelta { text } => {
                        let msg = self.message_mut()?;
                        match &mut msg.content[idx] {
                            ContentBlock::Text { text: existing } => existing.push_str(text),
                            other => {
                                *other = ContentBlock::Text { text: text.clone() };
                            }
                        }
                    }
                    ContentDelta::InputJsonDelta { partial_json } => {
                        self.tool_json[idx].push_str(partial_json);
                    }
                }
            }
            MessageStreamEvent::ContentBlockStop { index } => {
                // Finalize a tool_use block: parse its accumulated JSON.
                let idx = *index;
                if let Some(buf) = self.tool_json.get(idx).cloned() {
                    if !buf.is_empty() {
                        let parsed = serde_json::from_str(&buf).unwrap_or(serde_json::Value::Null);
                        let msg = self.message_mut()?;
                        if let Some(ContentBlock::ToolUse { input, .. }) = msg.content.get_mut(idx) {
                            *input = parsed;
                        }
                    }
                }
            }
            MessageStreamEvent::MessageDelta { delta, usage } => {
                let msg = self.message_mut()?;
                if delta.stop_reason.is_some() {
                    msg.stop_reason = delta.stop_reason;
                }
                if delta.stop_sequence.is_some() {
                    msg.stop_sequence = delta.stop_sequence.clone();
                }
                msg.usage.output_tokens = usage.output_tokens;
            }
            MessageStreamEvent::MessageStop
            | MessageStreamEvent::Ping
            | MessageStreamEvent::Error { .. } => {}
        }
        Ok(())
    }

    /// Take the accumulated [`Message`], erroring if no `message_start` was seen.
    pub fn into_message(self) -> Result<Message> {
        self.message
            .ok_or_else(|| Error::Stream("stream produced no message_start event".into()))
    }

    /// Borrow the in-progress message, if any.
    pub fn current(&self) -> Option<&Message> {
        self.message.as_ref()
    }

    fn message_mut(&mut self) -> Result<&mut Message> {
        self.message
            .as_mut()
            .ok_or_else(|| Error::Stream("received content event before message_start".into()))
    }
}

/// Grow a vec to at least `index + 1` elements, filling with `default`.
fn grow<T: Clone>(v: &mut Vec<T>, index: usize, default: T) {
    if v.len() <= index {
        v.resize(index + 1, default);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::StopReason;

    fn ev(json: &str) -> MessageStreamEvent {
        serde_json::from_str(json).expect("valid event json")
    }

    #[test]
    fn parses_named_event_frame() {
        let mut d = SseDecoder::new();
        let events =
            d.push(b"event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "message_stop");
        assert_eq!(events[0].data, "{\"type\":\"message_stop\"}");
    }

    #[test]
    fn parses_split_chunks() {
        let mut d = SseDecoder::new();
        assert!(d.push(b"event: ping\nda").is_empty());
        let events = d.push(b"ta: {\"type\":\"ping\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "ping");
    }

    #[test]
    fn handles_crlf() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: [DONE]\r\n\r\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "[DONE]");
    }

    #[test]
    fn anonymous_data_line() {
        let mut d = SseDecoder::new();
        let events = d.push(b"data: {\"type\":\"delta\",\"delta\":\"hi\"}\n\n");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event, "");
        assert!(events[0].data.contains("delta"));
    }

    #[test]
    fn ignores_comments() {
        let mut d = SseDecoder::new();
        let events = d.push(b": keepalive\n\n");
        assert!(events.is_empty());
    }

    #[test]
    fn accumulates_text_message() {
        let mut acc = MessageAccumulator::new();
        acc.apply(&ev(
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","model":"zoysia","content":[],"stop_reason":null,"stop_sequence":null,"usage":{"input_tokens":12,"output_tokens":0}}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Hello"}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":", world"}}"#,
        ))
        .unwrap();
        acc.apply(&ev(r#"{"type":"content_block_stop","index":0}"#)).unwrap();
        acc.apply(&ev(
            r#"{"type":"message_delta","delta":{"stop_reason":"end_turn","stop_sequence":null},"usage":{"output_tokens":34}}"#,
        ))
        .unwrap();
        acc.apply(&ev(r#"{"type":"message_stop"}"#)).unwrap();

        let msg = acc.into_message().unwrap();
        assert_eq!(msg.id, "msg_1");
        assert_eq!(msg.text(), "Hello, world");
        assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
        assert_eq!(msg.usage.input_tokens, 12);
        assert_eq!(msg.usage.output_tokens, 34);
    }

    #[test]
    fn accumulates_tool_use_from_json_deltas() {
        let mut acc = MessageAccumulator::new();
        acc.apply(&ev(
            r#"{"type":"message_start","message":{"id":"msg_2","type":"message","role":"assistant","model":"zoysia","content":[],"usage":{"input_tokens":5,"output_tokens":0}}}"#,
        ))
        .unwrap();
        // text block 0
        acc.apply(&ev(
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"Let me check."}}"#,
        ))
        .unwrap();
        acc.apply(&ev(r#"{"type":"content_block_stop","index":0}"#)).unwrap();
        // tool_use block 1, input streamed as json fragments
        acc.apply(&ev(
            r#"{"type":"content_block_start","index":1,"content_block":{"type":"tool_use","id":"toolu_9","name":"get_weather","input":{}}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#,
        ))
        .unwrap();
        acc.apply(&ev(
            r#"{"type":"content_block_delta","index":1,"delta":{"type":"input_json_delta","partial_json":"\"SF\"}"}}"#,
        ))
        .unwrap();
        acc.apply(&ev(r#"{"type":"content_block_stop","index":1}"#)).unwrap();
        acc.apply(&ev(
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use","stop_sequence":null},"usage":{"output_tokens":20}}"#,
        ))
        .unwrap();

        let msg = acc.into_message().unwrap();
        assert_eq!(msg.text(), "Let me check.");
        assert_eq!(msg.stop_reason, Some(StopReason::ToolUse));
        let tools = msg.tool_uses();
        assert_eq!(tools.len(), 1);
        match tools[0] {
            ContentBlock::ToolUse { name, input, id } => {
                assert_eq!(name, "get_weather");
                assert_eq!(id, "toolu_9");
                assert_eq!(input["city"], "SF");
            }
            _ => panic!("expected tool_use"),
        }
    }

    #[test]
    fn content_before_start_errors() {
        let mut acc = MessageAccumulator::new();
        let res = acc.apply(&ev(
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"x"}}"#,
        ));
        assert!(res.is_err());
    }
}
