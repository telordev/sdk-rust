//! Typed request/response models for the Simse API.
//!
//! The wire shapes mirror the Anthropic Messages API exactly (see
//! `sdk/CONTRACT.md`). Content blocks and stream events are serde-tagged enums
//! discriminated on the `type` field. JSON field names are snake_case on the
//! wire; the Rust idioms are snake_case too.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ════════════════════════════════════════════════════════════════════════════
// Messages — request
// ════════════════════════════════════════════════════════════════════════════

/// A conversation role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// A user (or tool-result) turn.
    User,
    /// An assistant turn.
    Assistant,
}

/// One conversation message. `content` is either a plain string or a list of
/// typed content blocks (untagged on the wire — a bare string OR a `[block]`
/// array, exactly as Anthropic accepts).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputMessage {
    /// The role of this turn.
    pub role: Role,
    /// The content (string or block array).
    pub content: Content,
}

impl InputMessage {
    /// A user turn from a plain string.
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: Content::Text(text.into()),
        }
    }

    /// An assistant turn from a plain string.
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: Content::Text(text.into()),
        }
    }

    /// A turn carrying explicit content blocks.
    pub fn blocks(role: Role, blocks: Vec<ContentBlock>) -> Self {
        Self {
            role,
            content: Content::Blocks(blocks),
        }
    }
}

/// Message content: a plain string or a list of blocks. Serialized **untagged**
/// so a string stays a string and a list stays a list on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Content {
    /// Plain text content.
    Text(String),
    /// A list of typed content blocks.
    Blocks(Vec<ContentBlock>),
}

impl From<String> for Content {
    fn from(s: String) -> Self {
        Content::Text(s)
    }
}
impl From<&str> for Content {
    fn from(s: &str) -> Self {
        Content::Text(s.to_string())
    }
}
impl From<Vec<ContentBlock>> for Content {
    fn from(b: Vec<ContentBlock>) -> Self {
        Content::Blocks(b)
    }
}

/// A typed content block — discriminated on `type`. Covers the input and output
/// block kinds from the contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// A text block.
    Text {
        /// The text.
        text: String,
    },
    /// An image block (input only); base64-sourced.
    Image {
        /// The image source.
        source: ImageSource,
    },
    /// A tool-use block — the model's request to call a tool (assistant output;
    /// also echoed back on a follow-up assistant turn).
    ToolUse {
        /// The tool-use id (`toolu_…`).
        id: String,
        /// The tool name.
        name: String,
        /// The tool input arguments.
        input: Value,
    },
    /// A tool-result block — a user turn returning a tool's output.
    ToolResult {
        /// The id of the `tool_use` this answers.
        tool_use_id: String,
        /// The result content (string or blocks).
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<Content>,
        /// Whether the tool errored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        is_error: Option<bool>,
    },
    /// A thinking block (echoed as context).
    Thinking {
        /// The thinking text.
        thinking: String,
    },
}

impl ContentBlock {
    /// Convenience: a text block.
    pub fn text(text: impl Into<String>) -> Self {
        ContentBlock::Text { text: text.into() }
    }

    /// Convenience: a base64 image block.
    pub fn image_base64(media_type: impl Into<String>, data: impl Into<String>) -> Self {
        ContentBlock::Image {
            source: ImageSource::Base64 {
                media_type: media_type.into(),
                data: data.into(),
            },
        }
    }

    /// Convenience: a tool-result block answering `tool_use_id`.
    pub fn tool_result(tool_use_id: impl Into<String>, content: impl Into<Content>) -> Self {
        ContentBlock::ToolResult {
            tool_use_id: tool_use_id.into(),
            content: Some(content.into()),
            is_error: None,
        }
    }

    /// The text of this block, if it is a text block.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            ContentBlock::Text { text } => Some(text),
            _ => None,
        }
    }
}

/// An image source. Only base64 is supported by the gateway.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ImageSource {
    /// Base64-encoded image bytes.
    Base64 {
        /// The MIME type (e.g. `image/png`).
        media_type: String,
        /// The base64 payload.
        data: String,
    },
}

/// A system prompt: a plain string or a list of text blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum System {
    /// A plain string system prompt.
    Text(String),
    /// A list of text blocks.
    Blocks(Vec<ContentBlock>),
}

impl From<&str> for System {
    fn from(s: &str) -> Self {
        System::Text(s.to_string())
    }
}
impl From<String> for System {
    fn from(s: String) -> Self {
        System::Text(s)
    }
}

/// A tool definition the model may call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tool {
    /// The tool name.
    pub name: String,
    /// An optional natural-language description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// The JSON Schema for the tool's input.
    pub input_schema: Value,
}

impl Tool {
    /// Build a tool from its name + JSON-Schema input.
    pub fn new(name: impl Into<String>, input_schema: Value) -> Self {
        Self {
            name: name.into(),
            description: None,
            input_schema,
        }
    }

    /// Attach a description.
    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// How the model should choose tools.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    /// The model decides whether to call a tool.
    Auto,
    /// The model must call some tool.
    Any,
    /// The model must call the named tool.
    Tool {
        /// The tool name to force.
        name: String,
    },
    /// The model must not call any tool.
    None,
}

/// Request metadata (opaque to the gateway).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Metadata {
    /// An opaque end-user identifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,
}

/// The `POST /v1/messages` request body. Use [`crate::MessageCreateBuilder`]
/// for an ergonomic builder.
#[derive(Debug, Clone, Serialize)]
pub struct MessageCreateParams {
    /// The model id (`"rye"` or `"zoysia"`).
    pub model: String,
    /// The conversation (non-empty).
    pub messages: Vec<InputMessage>,
    /// Max tokens to generate.
    pub max_tokens: u32,
    /// Optional system prompt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system: Option<System>,
    /// Sampling temperature (0..1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,
    /// Nucleus sampling (0..1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,
    /// Top-k sampling.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_k: Option<u32>,
    /// Stop sequences.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_sequences: Option<Vec<String>>,
    /// Whether to stream (set automatically by `.stream()`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
    /// Tool definitions.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<Tool>>,
    /// Tool-choice policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    /// Opaque metadata.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Metadata>,
}

impl MessageCreateParams {
    /// Start a builder for `model` + `max_tokens`.
    pub fn builder(model: impl Into<String>, max_tokens: u32) -> crate::MessageCreateBuilder {
        crate::MessageCreateBuilder::new(model, max_tokens)
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Messages — response
// ════════════════════════════════════════════════════════════════════════════

/// Why the model stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model reached a natural stopping point.
    EndTurn,
    /// `max_tokens` was hit.
    MaxTokens,
    /// A stop sequence matched.
    StopSequence,
    /// The model emitted a tool call.
    ToolUse,
}

/// Token usage for a message.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    /// Input tokens (an estimate on `message_start`, authoritative at the end).
    #[serde(default)]
    pub input_tokens: u64,
    /// Output tokens.
    #[serde(default)]
    pub output_tokens: u64,
}

/// A `Message` object — the non-streaming response and the accumulated result
/// of a stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// The message id (`msg_…`).
    pub id: String,
    /// Always `"message"`.
    #[serde(rename = "type")]
    pub kind: String,
    /// Always `Role::Assistant`.
    pub role: Role,
    /// The serving model.
    pub model: String,
    /// The output content blocks.
    pub content: Vec<ContentBlock>,
    /// Why the model stopped (absent mid-stream).
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
    /// The matched stop sequence, if any.
    #[serde(default)]
    pub stop_sequence: Option<String>,
    /// Token usage.
    #[serde(default)]
    pub usage: Usage,
}

impl Message {
    /// Concatenate all text blocks into a single string (the common-case
    /// convenience for non-tool responses).
    pub fn text(&self) -> String {
        self.content
            .iter()
            .filter_map(ContentBlock::as_text)
            .collect::<Vec<_>>()
            .join("")
    }

    /// All `tool_use` blocks in the response.
    pub fn tool_uses(&self) -> Vec<&ContentBlock> {
        self.content
            .iter()
            .filter(|b| matches!(b, ContentBlock::ToolUse { .. }))
            .collect()
    }
}

/// The `POST /v1/messages/count_tokens` response.
#[derive(Debug, Clone, Deserialize)]
pub struct TokenCount {
    /// The (estimated) input token count.
    pub input_tokens: u64,
}

// ════════════════════════════════════════════════════════════════════════════
// Streaming events (named SSE events, discriminated on `type`)
// ════════════════════════════════════════════════════════════════════════════

/// A typed Messages-API stream event (the named SSE event sequence).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageStreamEvent {
    /// The opening event — a `Message` skeleton with empty content.
    MessageStart {
        /// The initial message (empty content; estimated input tokens).
        message: Message,
    },
    /// A content block begins at `index`.
    ContentBlockStart {
        /// The block index.
        index: usize,
        /// The initial (empty) block.
        content_block: ContentBlock,
    },
    /// An incremental delta to the block at `index`.
    ContentBlockDelta {
        /// The block index.
        index: usize,
        /// The delta payload.
        delta: ContentDelta,
    },
    /// The block at `index` is complete.
    ContentBlockStop {
        /// The block index.
        index: usize,
    },
    /// Top-level message delta — stop reason + cumulative output usage.
    MessageDelta {
        /// The stop-reason delta.
        delta: MessageDeltaBody,
        /// Cumulative output usage.
        #[serde(default)]
        usage: MessageDeltaUsage,
    },
    /// The final event.
    MessageStop,
    /// A keepalive ping.
    Ping,
    /// A mid-stream error frame (`event: error`).
    Error {
        /// The error payload.
        error: StreamError,
    },
}

/// A delta inside a content block.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentDelta {
    /// Appended text.
    TextDelta {
        /// The text fragment.
        text: String,
    },
    /// A fragment of streamed tool-input JSON.
    InputJsonDelta {
        /// The partial JSON fragment (concatenate to reconstruct the input).
        partial_json: String,
    },
}

/// The `message_delta.delta` body.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageDeltaBody {
    /// The final stop reason.
    #[serde(default)]
    pub stop_reason: Option<StopReason>,
    /// The matched stop sequence.
    #[serde(default)]
    pub stop_sequence: Option<String>,
}

/// The `message_delta.usage` body (output tokens only).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MessageDeltaUsage {
    /// Cumulative output tokens.
    #[serde(default)]
    pub output_tokens: u64,
}

/// A mid-stream error payload.
#[derive(Debug, Clone, Deserialize)]
pub struct StreamError {
    /// The error type discriminator.
    #[serde(rename = "type", default)]
    pub error_type: String,
    /// The error message.
    #[serde(default)]
    pub message: String,
}

// ════════════════════════════════════════════════════════════════════════════
// Models
// ════════════════════════════════════════════════════════════════════════════

/// A hosted model (Anthropic `Model` object shape).
#[derive(Debug, Clone, Deserialize)]
pub struct Model {
    /// The model id (`"rye"` / `"zoysia"`).
    pub id: String,
    /// Always `"model"`.
    #[serde(rename = "type", default)]
    pub kind: String,
    /// A human display name.
    #[serde(default)]
    pub display_name: String,
    /// Creation timestamp (RFC 3339).
    #[serde(default)]
    pub created_at: String,
    /// Maximum input context.
    #[serde(default)]
    pub max_input_tokens: Option<u64>,
    /// Maximum output tokens.
    #[serde(default)]
    pub max_tokens: Option<u64>,
}

/// A page of models (`GET /v1/models`). The hosted catalog is small, so
/// `has_more` is currently always `false`.
#[derive(Debug, Clone, Deserialize)]
pub struct ModelList {
    /// The models on this page.
    pub data: Vec<Model>,
    /// Whether more pages exist.
    #[serde(default)]
    pub has_more: bool,
    /// The first id on this page (cursor).
    #[serde(default)]
    pub first_id: Option<String>,
    /// The last id on this page (cursor).
    #[serde(default)]
    pub last_id: Option<String>,
}

// ════════════════════════════════════════════════════════════════════════════
// Account / usage / billing
// ════════════════════════════════════════════════════════════════════════════

/// `GET /v1/account`.
#[derive(Debug, Clone, Deserialize)]
pub struct Account {
    /// The platform-account (or user) id.
    pub id: String,
    /// The owning user id.
    #[serde(default)]
    pub user_id: String,
    /// The plan tier.
    #[serde(default)]
    pub plan: String,
}

/// `GET /v1/usage`.
#[derive(Debug, Clone, Deserialize)]
pub struct Usage_ {
    /// The reporting period.
    #[serde(default)]
    pub period: String,
    /// Request count.
    #[serde(default)]
    pub requests: u64,
    /// Token count.
    #[serde(default)]
    pub tokens: u64,
    /// Per-model token breakdown.
    #[serde(default)]
    pub by_model: Value,
}

/// `GET /v1/billing`.
#[derive(Debug, Clone, Deserialize)]
pub struct Billing {
    /// The plan tier.
    #[serde(default)]
    pub plan: String,
    /// The subscription status.
    #[serde(default)]
    pub status: String,
    /// Plan limits (opaque).
    #[serde(default)]
    pub limits: Value,
    /// Current-period usage (opaque).
    #[serde(default)]
    pub current_usage: Value,
}

// ════════════════════════════════════════════════════════════════════════════
// Usage dashboard (`GET /v1/usage/dashboard`) — the rich UsagePanel view
// ════════════════════════════════════════════════════════════════════════════

/// `GET /v1/usage/dashboard` → the rich consumer **UsagePanel** view.
///
/// warp assembles this from the surviving authorities (payments + tally + core
/// billing). The wire keys are **camelCase** exactly as warp emits them; the
/// `#[serde(rename = …)]` attributes map them onto the snake_case Rust fields.
/// Each block degrades to its zero-value default independently, so the panel
/// always deserializes even when a downstream is unbound.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageDashboard {
    /// The plan tier (defaults to `"free"` when payments is unbound).
    #[serde(default)]
    pub plan: String,
    /// The current billing-period start, in epoch milliseconds.
    #[serde(rename = "periodStart", default)]
    pub period_start: i64,
    /// Per-model included-vs-extra usage breakdown.
    #[serde(default)]
    pub models: Vec<UsageDashboardModel>,
    /// The billing block (extra-usage toggle/cap + credit balance + spend).
    #[serde(default)]
    pub billing: UsageDashboardBilling,
    /// The compute-session window (omitted when tally is unbound / has no row).
    #[serde(default)]
    pub compute: Option<UsageDashboardCompute>,
}

/// One per-model row of the usage dashboard (`models[]`). Token counts are the
/// included-vs-extra split for the tier this billing period; `multiplier` is the
/// tier's billing weight (core's `model_multiplier`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageDashboardModel {
    /// The model id (`"rye"` / `"zoysia"`).
    #[serde(default)]
    pub model: String,
    /// Input tokens billed within the plan's included allowance.
    #[serde(rename = "includedInputTokens", default)]
    pub included_input_tokens: i64,
    /// Output tokens billed within the plan's included allowance.
    #[serde(rename = "includedOutputTokens", default)]
    pub included_output_tokens: i64,
    /// Input tokens billed as paid overage.
    #[serde(rename = "extraInputTokens", default)]
    pub extra_input_tokens: i64,
    /// Output tokens billed as paid overage.
    #[serde(rename = "extraOutputTokens", default)]
    pub extra_output_tokens: i64,
    /// Overage spend attributed to this model, in cents.
    #[serde(rename = "extraSpendCents", default)]
    pub extra_spend_cents: i64,
    /// Number of requests against this model this period.
    #[serde(rename = "requestCount", default)]
    pub request_count: i64,
    /// The tier's billing multiplier.
    #[serde(default)]
    pub multiplier: f64,
}

/// The `billing` block of the usage dashboard.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageDashboardBilling {
    /// Whether paid overage beyond the included allowance is enabled.
    #[serde(rename = "extraUsageEnabled", default)]
    pub extra_usage_enabled: bool,
    /// The spend cap for paid overage this period, in cents.
    #[serde(rename = "extraUsageCapCents", default)]
    pub extra_usage_cap_cents: i64,
    /// The remaining prepaid credit balance, in cents.
    #[serde(rename = "creditsBalanceCents", default)]
    pub credits_balance_cents: i64,
    /// Overage spend so far this period, in cents.
    #[serde(rename = "extraSpendThisPeriodCents", default)]
    pub extra_spend_this_period_cents: i64,
    /// The plan's included token allowance.
    #[serde(rename = "planIncludedTokens", default)]
    pub plan_included_tokens: i64,
}

/// The `compute` block of the usage dashboard (the 5h compute-session window).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageDashboardCompute {
    /// The session sub-block.
    #[serde(default)]
    pub session: Option<UsageDashboardSession>,
}

/// The `compute.session` sub-block.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct UsageDashboardSession {
    /// The current compute-session window (`null` when there is no row).
    #[serde(default)]
    pub current: Option<ComputeSessionWindow>,
}

/// The `compute.session.current` window — the active/cooldown/ready 5h timer.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ComputeSessionWindow {
    /// The derived window state (`"active"` / `"cooldown"` / `"ready"`).
    #[serde(default)]
    pub state: String,
    /// When the window started, in epoch milliseconds.
    #[serde(rename = "startedAt", default)]
    pub started_at: i64,
    /// When the active window expires, in epoch milliseconds.
    #[serde(rename = "expiresAt", default)]
    pub expires_at: i64,
    /// When the cooldown lifts, in epoch milliseconds.
    #[serde(rename = "cooldownUntil", default)]
    pub cooldown_until: i64,
    /// Time used within the active window, in milliseconds.
    #[serde(rename = "usedMs", default)]
    pub used_ms: i64,
    /// The active-window length, in milliseconds.
    #[serde(rename = "limitMs", default)]
    pub limit_ms: i64,
    /// In-flight prompts (always `0` — the in-flight count no longer exists).
    #[serde(rename = "inFlight", default)]
    pub in_flight: i64,
}

// ════════════════════════════════════════════════════════════════════════════
// Agents (`GET /v1/agents`) — subagent run history
// ════════════════════════════════════════════════════════════════════════════

/// One subagent run record (`GET /v1/agents` → `agents[]`).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentRun {
    /// The subagent run id.
    #[serde(default)]
    pub id: String,
    /// The task description the subagent was dispatched with.
    #[serde(default)]
    pub description: String,
    /// The run status (e.g. `"running"` / `"completed"` / `"failed"`).
    #[serde(default)]
    pub status: String,
    /// When the run started (timestamp, server-formatted).
    #[serde(default)]
    pub started_at: String,
    /// When the run completed (empty while still running).
    #[serde(default)]
    pub completed_at: String,
    /// Wall-clock duration, in milliseconds.
    #[serde(default)]
    pub duration_ms: i64,
    /// Number of model turns the run took.
    #[serde(default)]
    pub turns: i64,
    /// Input (prompt) tokens consumed.
    #[serde(default)]
    pub input_tokens: i64,
    /// Output (completion) tokens produced.
    #[serde(default)]
    pub output_tokens: i64,
    /// The error message when the run failed (empty otherwise).
    #[serde(default)]
    pub error: String,
}

/// `GET /v1/agents` envelope — subagent run history, newest-first.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentList {
    /// The subagent runs.
    #[serde(default)]
    pub agents: Vec<AgentRun>,
}

// ════════════════════════════════════════════════════════════════════════════
// Sessions
// ════════════════════════════════════════════════════════════════════════════

/// A created session (`POST /v1/sessions`).
#[derive(Debug, Clone, Deserialize)]
pub struct Session {
    /// The session id.
    pub id: String,
    /// The model (if pinned).
    #[serde(default)]
    pub model: Option<String>,
    /// The session status.
    #[serde(default)]
    pub status: String,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: String,
}

/// A session summary in a list (`GET /v1/sessions`).
#[derive(Debug, Clone, Deserialize)]
pub struct SessionSummary {
    /// The session id.
    pub id: String,
    /// The title.
    #[serde(default)]
    pub title: String,
    /// Number of messages.
    #[serde(default)]
    pub message_count: u64,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: String,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: String,
    /// The status.
    #[serde(default)]
    pub status: String,
}

/// `GET /v1/sessions` envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionList {
    /// The sessions.
    #[serde(default)]
    pub sessions: Vec<SessionSummary>,
}

/// The buffered prompt result (`POST /v1/sessions/{id}/messages`,
/// `stream:false`).
#[derive(Debug, Clone, Deserialize)]
pub struct SessionPromptResult {
    /// The assistant message.
    pub message: SessionMessage,
    /// Token usage.
    #[serde(default)]
    pub usage: SessionUsage,
}

/// An assistant message in a session prompt result.
#[derive(Debug, Clone, Deserialize)]
pub struct SessionMessage {
    /// The role (`"assistant"`).
    #[serde(default)]
    pub role: String,
    /// The content text.
    #[serde(default)]
    pub content: String,
}

/// Session-prompt token usage (`{input_tokens, output_tokens}`, matching the
/// gateway wire). There is no `total_tokens` field on the wire; use
/// [`SessionUsage::total_tokens`] to compute it.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct SessionUsage {
    /// Input (prompt) tokens.
    #[serde(default)]
    pub input_tokens: u64,
    /// Output (completion) tokens.
    #[serde(default)]
    pub output_tokens: u64,
}

impl SessionUsage {
    /// The total token count (`input_tokens + output_tokens`).
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}

/// A streaming session-prompt event (the agentic SSE shape — anonymous
/// `data: {json}` lines, discriminated on `type`).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SessionStreamEvent {
    /// An incremental text delta.
    Delta {
        /// The text fragment.
        delta: String,
    },
    /// The model invoked a tool.
    ToolCall {
        /// The call id.
        id: String,
        /// The tool name.
        name: String,
        /// The tool input.
        #[serde(default)]
        input: Value,
    },
    /// A tool returned a result.
    ToolResult {
        /// The call id.
        id: String,
        /// The tool output.
        #[serde(default)]
        output: Value,
        /// Whether the tool errored.
        #[serde(default)]
        is_error: bool,
    },
    /// The terminal frame.
    Done {
        /// The final status (e.g. `"complete"`).
        #[serde(default)]
        status: String,
        /// The authoritative final text.
        #[serde(default)]
        text: String,
        /// Token usage.
        #[serde(default)]
        usage: SessionUsage,
    },
    /// A mid-stream error.
    Error {
        /// The error message.
        #[serde(default)]
        message: String,
    },
}

/// The reconstructed history from `POST /v1/sessions/{id}/resume`.
#[derive(Debug, Clone, Deserialize)]
pub struct ResumedSession {
    /// The message history.
    #[serde(default)]
    pub messages: Vec<Value>,
    /// The tool-call records.
    #[serde(default)]
    pub tool_calls: Vec<Value>,
}

// ════════════════════════════════════════════════════════════════════════════
// Memories
// ════════════════════════════════════════════════════════════════════════════

// ════════════════════════════════════════════════════════════════════════════
// Connectors
// ════════════════════════════════════════════════════════════════════════════

/// A registered connector (remote-MCP server). Bearer and header values are
/// **always redacted** in API responses (spec §1.3); the `auth` field is
/// present but its `value` is `null`.
#[derive(Debug, Clone, Deserialize)]
pub struct Connector {
    /// The connector id.
    pub id: String,
    /// The owning user id.
    #[serde(default)]
    pub user_id: String,
    /// A human-readable name.
    #[serde(default)]
    pub name: String,
    /// The connector type (always `"mcp"`).
    #[serde(rename = "type", default)]
    pub kind: String,
    /// The MCP endpoint URL.
    #[serde(default)]
    pub url: String,
    /// Auth metadata (value redacted).
    #[serde(default)]
    pub auth: Option<serde_json::Value>,
    /// Static extra headers (values redacted).
    #[serde(default)]
    pub headers: Option<serde_json::Value>,
    /// Tool allowlist, if set.
    #[serde(default)]
    pub tool_allowlist: Option<Vec<String>>,
    /// Tool denylist, if set.
    #[serde(default)]
    pub tool_denylist: Option<Vec<String>>,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: String,
    /// Last-updated timestamp.
    #[serde(default)]
    pub updated_at: String,
}

/// `GET /v1/connectors` envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectorList {
    /// The connectors.
    #[serde(default)]
    pub connectors: Vec<Connector>,
}

/// `POST /v1/connectors/{id}/test` response.
#[derive(Debug, Clone, Deserialize)]
pub struct ConnectorTestResult {
    /// Whether `tools/list` succeeded.
    pub ok: bool,
    /// Number of tools returned (0 when `ok` is false).
    #[serde(default)]
    pub tool_count: u32,
    /// Error message when `ok` is false.
    #[serde(default)]
    pub error: Option<String>,
}

/// A memory entry.
#[derive(Debug, Clone, Deserialize)]
pub struct Memory {
    /// The memory id.
    #[serde(default)]
    pub id: String,
    /// The stored text.
    #[serde(default)]
    pub text: String,
    /// Arbitrary metadata.
    #[serde(default)]
    pub metadata: Value,
    /// Creation timestamp.
    #[serde(default)]
    pub created_at: Value,
    /// Search relevance score (when searching).
    #[serde(default)]
    pub score: Option<f64>,
}

/// `GET /v1/memories` envelope.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryList {
    /// The memories.
    #[serde(default)]
    pub memories: Vec<Memory>,
}

/// `GET /v1/memories/stats`.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryStats {
    /// The number of stored memories.
    #[serde(default)]
    pub count: u64,
    /// The most recent add timestamp.
    #[serde(default)]
    pub last_added_at: Value,
}

/// The id returned by `POST /v1/memories`.
#[derive(Debug, Clone, Deserialize)]
pub struct CreatedMemory {
    /// The new memory id.
    pub id: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The usage-dashboard deserializes warp's camelCase wire keys onto the
    /// snake_case Rust fields — every block, including the nested compute window.
    #[test]
    fn usage_dashboard_full_camelcase_roundtrip() {
        let wire = serde_json::json!({
            "plan": "pro",
            "periodStart": 1_750_000_000_000_i64,
            "models": [{
                "model": "zoysia",
                "includedInputTokens": 1000,
                "includedOutputTokens": 2000,
                "extraInputTokens": 30,
                "extraOutputTokens": 40,
                "extraSpendCents": 12,
                "requestCount": 7,
                "multiplier": 2.5,
            }],
            "billing": {
                "extraUsageEnabled": true,
                "extraUsageCapCents": 5000,
                "creditsBalanceCents": 250,
                "extraSpendThisPeriodCents": 12,
                "planIncludedTokens": 1_000_000_i64,
            },
            "compute": {
                "session": {
                    "current": {
                        "state": "active",
                        "startedAt": 100,
                        "expiresAt": 200,
                        "cooldownUntil": 300,
                        "usedMs": 3_600_000_i64,
                        "limitMs": 18_000_000_i64,
                        "inFlight": 0,
                    }
                }
            }
        });

        let d: UsageDashboard = serde_json::from_value(wire).unwrap();
        assert_eq!(d.plan, "pro");
        assert_eq!(d.period_start, 1_750_000_000_000);
        assert_eq!(d.models.len(), 1);

        let m = &d.models[0];
        assert_eq!(m.model, "zoysia");
        assert_eq!(m.included_input_tokens, 1000);
        assert_eq!(m.included_output_tokens, 2000);
        assert_eq!(m.extra_input_tokens, 30);
        assert_eq!(m.extra_output_tokens, 40);
        assert_eq!(m.extra_spend_cents, 12);
        assert_eq!(m.request_count, 7);
        assert_eq!(m.multiplier, 2.5);

        assert!(d.billing.extra_usage_enabled);
        assert_eq!(d.billing.extra_usage_cap_cents, 5000);
        assert_eq!(d.billing.credits_balance_cents, 250);
        assert_eq!(d.billing.extra_spend_this_period_cents, 12);
        assert_eq!(d.billing.plan_included_tokens, 1_000_000);

        let win = d
            .compute
            .unwrap()
            .session
            .unwrap()
            .current
            .unwrap();
        assert_eq!(win.state, "active");
        assert_eq!(win.started_at, 100);
        assert_eq!(win.expires_at, 200);
        assert_eq!(win.cooldown_until, 300);
        assert_eq!(win.used_ms, 3_600_000);
        assert_eq!(win.limit_ms, 18_000_000);
        assert_eq!(win.in_flight, 0);
    }

    /// When tally is unbound warp emits `compute: null` and the degraded
    /// defaults (free plan, empty models, zeroed billing) — the typed struct
    /// tolerates the omission rather than failing.
    #[test]
    fn usage_dashboard_omitted_compute_degrades() {
        let wire = serde_json::json!({
            "plan": "free",
            "periodStart": 0,
            "models": [],
            "billing": {
                "extraUsageEnabled": false,
                "extraUsageCapCents": 0,
                "creditsBalanceCents": 0,
                "extraSpendThisPeriodCents": 0,
                "planIncludedTokens": 200_000,
            },
            "compute": null
        });
        let d: UsageDashboard = serde_json::from_value(wire).unwrap();
        assert_eq!(d.plan, "free");
        assert!(d.models.is_empty());
        assert!(!d.billing.extra_usage_enabled);
        assert_eq!(d.billing.plan_included_tokens, 200_000);
        assert!(d.compute.is_none());
    }

    /// The agents envelope deserializes warp's `agents[]` records with all the
    /// run fields.
    #[test]
    fn agent_list_deserializes() {
        let wire = serde_json::json!({
            "agents": [{
                "id": "agent-1",
                "description": "refactor the auth gate",
                "status": "completed",
                "started_at": "2026-06-19T00:00:00Z",
                "completed_at": "2026-06-19T00:05:00Z",
                "duration_ms": 300_000,
                "turns": 4,
                "input_tokens": 1200,
                "output_tokens": 800,
                "error": "",
            }]
        });
        let list: AgentList = serde_json::from_value(wire).unwrap();
        assert_eq!(list.agents.len(), 1);
        let a = &list.agents[0];
        assert_eq!(a.id, "agent-1");
        assert_eq!(a.description, "refactor the auth gate");
        assert_eq!(a.status, "completed");
        assert_eq!(a.started_at, "2026-06-19T00:00:00Z");
        assert_eq!(a.completed_at, "2026-06-19T00:05:00Z");
        assert_eq!(a.duration_ms, 300_000);
        assert_eq!(a.turns, 4);
        assert_eq!(a.input_tokens, 1200);
        assert_eq!(a.output_tokens, 800);
        assert_eq!(a.error, "");
    }

    /// A still-running agent has empty `completed_at`/`error` and the envelope
    /// tolerates a missing `agents` array (defaults to empty).
    #[test]
    fn agent_list_tolerates_partial_and_empty() {
        let running: AgentList = serde_json::from_value(serde_json::json!({
            "agents": [{ "id": "agent-2", "status": "running" }]
        }))
        .unwrap();
        assert_eq!(running.agents[0].status, "running");
        assert_eq!(running.agents[0].completed_at, "");
        assert_eq!(running.agents[0].duration_ms, 0);

        let empty: AgentList = serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(empty.agents.is_empty());
    }
}
