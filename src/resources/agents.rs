//! The Agents API (`GET /v1/agents`) — subagent run history.

use reqwest::Method;

use crate::client::Client;
use crate::error::Result;
use crate::types::AgentList;

/// The agents resource (`GET /v1/agents`).
#[derive(Clone)]
pub struct Agents {
    client: Client,
}

impl Agents {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/agents` → subagent run history (newest first), per-user scoped.
    ///
    /// Returns an [`AgentList`] with the recorded runs (id, description, status,
    /// timestamps, duration, turn + token counts, and any error).
    pub async fn list(&self) -> Result<AgentList> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/agents", None)
            .await
    }
}
