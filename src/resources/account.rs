//! Account / usage / billing read views.

use reqwest::Method;

use crate::client::Client;
use crate::error::Result;
use crate::types::{
    Account as AccountModel, Billing as BillingModel, Usage_ as UsageModel, UsageDashboard,
};

/// The account resource (`GET /v1/account`).
#[derive(Clone)]
pub struct Account {
    client: Client,
}

impl Account {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/account` → the caller's account profile.
    pub async fn retrieve(&self) -> Result<AccountModel> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/account", None)
            .await
    }
}

/// The usage resource (`GET /v1/usage`).
#[derive(Clone)]
pub struct Usage {
    client: Client,
}

impl Usage {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/usage` → period usage (requests, tokens, per-model breakdown).
    pub async fn retrieve(&self) -> Result<UsageModel> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/usage", None)
            .await
    }

    /// `GET /v1/usage/dashboard` → the rich consumer **UsagePanel** view
    /// ([`UsageDashboard`]): plan, period start, per-model included-vs-extra
    /// breakdown, the billing block (overage toggle/cap + credit balance), and
    /// the compute-session window. The wire keys are camelCase; the typed struct
    /// maps them onto snake_case fields via serde renames.
    pub async fn dashboard(&self) -> Result<UsageDashboard> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/usage/dashboard", None)
            .await
    }
}

/// The billing resource (`GET /v1/billing`).
#[derive(Clone)]
pub struct Billing {
    client: Client,
}

impl Billing {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/billing` → plan, status, limits, current usage.
    pub async fn retrieve(&self) -> Result<BillingModel> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/billing", None)
            .await
    }
}
