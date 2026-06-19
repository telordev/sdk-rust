//! Resource namespaces — one type per API surface, each holding a
//! [`Client`](crate::Client).

pub mod account;
pub mod agents;
pub mod flags;
pub mod memories;
pub mod messages;
pub mod models;
pub mod plugins;
pub mod pm;
pub mod sessions;

pub use account::{Account, Billing, Usage};
pub use agents::Agents;
pub use flags::Flags;
pub use memories::Memories;
pub use messages::{MessageStream, Messages};
pub use models::Models;
pub use plugins::Plugins;
pub use pm::Pm;
pub use sessions::{SessionPromptStream, Sessions};
