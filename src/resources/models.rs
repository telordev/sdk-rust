//! The Models API (`/v1/models`, `/v1/models/{id}`).

use reqwest::Method;

use crate::client::Client;
use crate::error::Result;
use crate::types::{Model, ModelList};

/// The Models resource.
#[derive(Clone)]
pub struct Models {
    client: Client,
}

impl Models {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// `GET /v1/models` → the model page (with cursor fields). The hosted
    /// catalog is small, so `has_more` is currently always `false`.
    pub async fn list(&self) -> Result<ModelList> {
        self.client
            .request_json::<(), _>(Method::GET, "/v1/models", None)
            .await
    }

    /// `GET /v1/models` with cursor / limit query parameters.
    pub async fn list_with(&self, params: ModelListParams) -> Result<ModelList> {
        let path = params.to_path();
        self.client
            .request_json::<(), _>(Method::GET, &path, None)
            .await
    }

    /// `GET /v1/models/{model_id}` → a single [`Model`] (404 → `NotFoundError`).
    pub async fn retrieve(&self, model_id: impl AsRef<str>) -> Result<Model> {
        let path = format!("/v1/models/{}", model_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }
}

/// Pagination / limit parameters for `models.list`.
#[derive(Debug, Clone, Default)]
pub struct ModelListParams {
    /// Page size (default 20 server-side).
    pub limit: Option<u32>,
    /// Return the page before this id.
    pub before_id: Option<String>,
    /// Return the page after this id.
    pub after_id: Option<String>,
}

impl ModelListParams {
    fn to_path(&self) -> String {
        let mut q: Vec<String> = Vec::new();
        if let Some(l) = self.limit {
            q.push(format!("limit={l}"));
        }
        if let Some(b) = &self.before_id {
            q.push(format!("before_id={b}"));
        }
        if let Some(a) = &self.after_id {
            q.push(format!("after_id={a}"));
        }
        if q.is_empty() {
            "/v1/models".to_string()
        } else {
            format!("/v1/models?{}", q.join("&"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_query_path() {
        let p = ModelListParams {
            limit: Some(5),
            after_id: Some("rye".into()),
            ..Default::default()
        };
        assert_eq!(p.to_path(), "/v1/models?limit=5&after_id=rye");
        assert_eq!(ModelListParams::default().to_path(), "/v1/models");
    }
}
