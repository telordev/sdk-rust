//! # simse — the official Rust SDK for the Simse API
//!
//! [`api.simse.dev`](https://api.simse.dev) is an Anthropic-Messages-compatible
//! LLM gateway. This crate mirrors the official Anthropic SDK ergonomics:
//! `client.messages().create(...)` / `.stream(...)` / `.count_tokens(...)`,
//! `client.models().list()` / `.retrieve(id)`, a typed error hierarchy, and
//! automatic retries with backoff.
//!
//! ## Quickstart
//!
//! ```no_run
//! use simse::{Client, MessageCreateParams, types::InputMessage};
//!
//! # async fn run() -> simse::Result<()> {
//! // Reads SIMSE_API_KEY (or ANTHROPIC_API_KEY) from the environment.
//! let client = Client::from_env()?;
//!
//! let message = client
//!     .messages()
//!     .create(
//!         MessageCreateParams::builder("zoysia", 1024)
//!             .system("You are a helpful assistant.")
//!             .message(InputMessage::user("Hello, Simse!"))
//!             .build(),
//!     )
//!     .await?;
//!
//! println!("{}", message.text());
//! # Ok(())
//! # }
//! ```
//!
//! ## Streaming
//!
//! ```no_run
//! use futures::StreamExt;
//! use simse::{Client, MessageCreateParams, types::{InputMessage, MessageStreamEvent, ContentDelta}};
//!
//! # async fn run() -> simse::Result<()> {
//! let client = Client::from_env()?;
//! let mut stream = client
//!     .messages()
//!     .stream(
//!         MessageCreateParams::builder("zoysia", 1024)
//!             .message(InputMessage::user("Tell me a story."))
//!             .build(),
//!     )
//!     .await?;
//!
//! while let Some(event) = stream.next().await {
//!     if let MessageStreamEvent::ContentBlockDelta { delta: ContentDelta::TextDelta { text }, .. } = event? {
//!         print!("{text}");
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! See the `examples/` directory for tool-use, models, and session examples.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod auth;
mod builder;
mod client;
mod error;
#[doc(hidden)]
pub mod resources;
mod sse;
pub mod types;

pub use auth::{AuthProvider, SharedAuthProvider};
pub use builder::MessageCreateBuilder;
pub use client::{
    Client, ClientBuilder, ResponseHook, ResponseMeta, ANTHROPIC_VERSION, DEFAULT_BASE_URL,
    DEFAULT_MAX_RETRIES, DEFAULT_TIMEOUT,
};
pub use error::{ApiErrorBody, ApiErrorKind, Error, Result};
pub use types::MessageCreateParams;

// The SSE primitives are public so callers can build their own stream consumers
// and unit-test against the accumulator.
pub use sse::{MessageAccumulator, SseDecoder, SseEvent};

/// The resource namespace types (`messages`, `models`, `sessions`, …) and the
/// streaming response handles, accessed via the `Client::<resource>()` methods.
pub mod resource {
    pub use crate::resources::{
        Account, Agents, Billing, Flags, Memories, MessageStream, Messages, Models, Plugins, Pm,
        SessionPromptStream, Sessions, Usage,
    };
}

/// Request-parameter structs and response wrappers used by the resource methods.
pub mod params {
    pub use crate::resources::flags::FlagSet;
    pub use crate::resources::memories::{MemoryCreateParams, MemoryListParams};
    pub use crate::resources::models::ModelListParams;
    pub use crate::resources::plugins::InstallParams;
    pub use crate::resources::pm::{
        ProjectCreateParams, ProjectUpdateParams, Projects, ScheduleCreateParams,
        ScheduleUpdateParams, Schedules, TaskCreateParams, TaskUpdateParams, Tasks, Todos,
        WorkflowCreateParams, WorkflowUpdateParams, Workflows,
    };
    pub use crate::resources::sessions::SessionCreateParams;
}

// The flags response type is small + frequently checked; surface it at the root.
pub use resources::flags::FlagSet;
