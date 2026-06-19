//! An ergonomic builder for [`MessageCreateParams`].

use crate::types::{
    Content, ContentBlock, InputMessage, MessageCreateParams, Metadata, Role, System, Tool,
    ToolChoice,
};

/// Builder for a `POST /v1/messages` request.
///
/// ```no_run
/// use simse::{MessageCreateParams, types::InputMessage};
/// let params = MessageCreateParams::builder("zoysia", 1024)
///     .system("You are concise.")
///     .message(InputMessage::user("Hello!"))
///     .temperature(0.7)
///     .build();
/// ```
#[derive(Debug, Clone)]
pub struct MessageCreateBuilder {
    model: String,
    max_tokens: u32,
    messages: Vec<InputMessage>,
    system: Option<System>,
    temperature: Option<f64>,
    top_p: Option<f64>,
    top_k: Option<u32>,
    stop_sequences: Option<Vec<String>>,
    tools: Option<Vec<Tool>>,
    tool_choice: Option<ToolChoice>,
    metadata: Option<Metadata>,
}

impl MessageCreateBuilder {
    /// Start a builder for `model` + `max_tokens`.
    pub fn new(model: impl Into<String>, max_tokens: u32) -> Self {
        Self {
            model: model.into(),
            max_tokens,
            messages: Vec::new(),
            system: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            metadata: None,
        }
    }

    /// Append a message.
    pub fn message(mut self, m: InputMessage) -> Self {
        self.messages.push(m);
        self
    }

    /// Append a user-text turn.
    pub fn user(mut self, text: impl Into<String>) -> Self {
        self.messages.push(InputMessage::user(text));
        self
    }

    /// Append an assistant-text turn.
    pub fn assistant(mut self, text: impl Into<String>) -> Self {
        self.messages.push(InputMessage::assistant(text));
        self
    }

    /// Append a turn with explicit content blocks.
    pub fn message_blocks(mut self, role: Role, blocks: Vec<ContentBlock>) -> Self {
        self.messages.push(InputMessage {
            role,
            content: Content::Blocks(blocks),
        });
        self
    }

    /// Replace the whole message list.
    pub fn messages(mut self, messages: Vec<InputMessage>) -> Self {
        self.messages = messages;
        self
    }

    /// Set the system prompt.
    pub fn system(mut self, system: impl Into<System>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the sampling temperature.
    pub fn temperature(mut self, t: f64) -> Self {
        self.temperature = Some(t);
        self
    }

    /// Set nucleus sampling.
    pub fn top_p(mut self, p: f64) -> Self {
        self.top_p = Some(p);
        self
    }

    /// Set top-k sampling.
    pub fn top_k(mut self, k: u32) -> Self {
        self.top_k = Some(k);
        self
    }

    /// Set stop sequences.
    pub fn stop_sequences(mut self, seqs: Vec<String>) -> Self {
        self.stop_sequences = Some(seqs);
        self
    }

    /// Append a tool definition.
    pub fn tool(mut self, tool: Tool) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Replace the whole tool list.
    pub fn tools(mut self, tools: Vec<Tool>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Set the tool-choice policy.
    pub fn tool_choice(mut self, choice: ToolChoice) -> Self {
        self.tool_choice = Some(choice);
        self
    }

    /// Set request metadata.
    pub fn metadata(mut self, metadata: Metadata) -> Self {
        self.metadata = Some(metadata);
        self
    }

    /// Set the metadata `user_id`.
    pub fn user_id(mut self, user_id: impl Into<String>) -> Self {
        self.metadata
            .get_or_insert_with(Metadata::default)
            .user_id = Some(user_id.into());
        self
    }

    /// Finalize into [`MessageCreateParams`] (with `stream` unset).
    pub fn build(self) -> MessageCreateParams {
        MessageCreateParams {
            model: self.model,
            messages: self.messages,
            max_tokens: self.max_tokens,
            system: self.system,
            temperature: self.temperature,
            top_p: self.top_p,
            top_k: self.top_k,
            stop_sequences: self.stop_sequences,
            stream: None,
            tools: self.tools,
            tool_choice: self.tool_choice,
            metadata: self.metadata,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn builds_params() {
        let p = MessageCreateBuilder::new("zoysia", 256)
            .system("be nice")
            .user("hi")
            .temperature(0.5)
            .build();
        assert_eq!(p.model, "zoysia");
        assert_eq!(p.max_tokens, 256);
        assert_eq!(p.messages.len(), 1);
        assert_eq!(p.temperature, Some(0.5));
        assert!(p.stream.is_none());
    }

    #[test]
    fn serializes_to_wire_shape() {
        let p = MessageCreateBuilder::new("rye", 100)
            .system("sys")
            .user("hello")
            .tool(Tool::new("get_weather", json!({"type":"object"})).with_description("weather"))
            .build();
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(v["model"], "rye");
        assert_eq!(v["max_tokens"], 100);
        assert_eq!(v["system"], "sys");
        assert_eq!(v["messages"][0]["role"], "user");
        assert_eq!(v["messages"][0]["content"], "hello");
        assert_eq!(v["tools"][0]["name"], "get_weather");
        assert_eq!(v["tools"][0]["input_schema"]["type"], "object");
        // `stream` is omitted when unset.
        assert!(v.get("stream").is_none());
    }

    #[test]
    fn content_blocks_serialize_as_array() {
        let p = MessageCreateBuilder::new("rye", 50)
            .message_blocks(
                Role::User,
                vec![
                    ContentBlock::text("look at this"),
                    ContentBlock::image_base64("image/png", "AAAA"),
                ],
            )
            .build();
        let v = serde_json::to_value(&p).unwrap();
        let content = &v["messages"][0]["content"];
        assert!(content.is_array());
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "image");
        assert_eq!(content[1]["source"]["type"], "base64");
        assert_eq!(content[1]["source"]["media_type"], "image/png");
    }
}
