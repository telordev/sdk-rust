//! The project-management API: session-scoped tasks/projects/todos and
//! user-scoped schedules/workflows.
//!
//! PM bodies use camelCase JSON field names on the wire (matching the gateway's
//! slate proxy), so this module passes typed builder params that serialize to
//! the right shape. Responses are returned as [`serde_json::Value`] because the
//! PM payloads are large and the console-facing shapes evolve; the field names
//! are documented per method.

use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};

use crate::client::Client;
use crate::error::Result;

/// The PM resource, split into sub-namespaces.
#[derive(Clone)]
pub struct Pm {
    client: Client,
}

impl Pm {
    pub(crate) fn new(client: Client) -> Self {
        Self { client }
    }

    /// Session-scoped tasks (kanban board).
    pub fn tasks(&self) -> Tasks {
        Tasks {
            client: self.client.clone(),
        }
    }
    /// Session-scoped projects.
    pub fn projects(&self) -> Projects {
        Projects {
            client: self.client.clone(),
        }
    }
    /// Session-scoped todos (read-only).
    pub fn todos(&self) -> Todos {
        Todos {
            client: self.client.clone(),
        }
    }
    /// User-scoped cron schedules.
    pub fn schedules(&self) -> Schedules {
        Schedules {
            client: self.client.clone(),
        }
    }
    /// User-scoped workflows.
    pub fn workflows(&self) -> Workflows {
        Workflows {
            client: self.client.clone(),
        }
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Tasks (session-scoped)
// ════════════════════════════════════════════════════════════════════════════

/// Session-scoped task (kanban) operations.
#[derive(Clone)]
pub struct Tasks {
    client: Client,
}

/// Parameters for creating a task (camelCase wire fields).
#[derive(Debug, Clone, Serialize)]
pub struct TaskCreateParams {
    /// The task title.
    pub title: String,
    /// An optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Project to file under.
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Parent task (for subtasks).
    #[serde(rename = "parentTaskId", skip_serializing_if = "Option::is_none")]
    pub parent_task_id: Option<String>,
    /// Initial column/status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// Priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// Labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
}

impl TaskCreateParams {
    /// A task from a title.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            description: None,
            project_id: None,
            parent_task_id: None,
            status: None,
            priority: None,
            labels: None,
        }
    }
}

/// Parameters for updating a task (all optional).
#[derive(Debug, Clone, Default, Serialize)]
pub struct TaskUpdateParams {
    /// New title.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// New description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// New status.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    /// New priority.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub priority: Option<String>,
    /// New labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub labels: Option<Vec<String>>,
    /// New project.
    #[serde(rename = "projectId", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl Tasks {
    /// `GET /v1/sessions/{id}/tasks` → the full board (tasks/projects/deps).
    pub async fn list(&self, session_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/sessions/{}/tasks", session_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `GET /v1/sessions/{id}/tasks/{task_id}` → one task + subtasks + deps.
    pub async fn retrieve(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
    ) -> Result<Value> {
        let path = format!("/v1/sessions/{}/tasks/{}", session_id.as_ref(), task_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `POST /v1/sessions/{id}/tasks` → create. Returns `{taskId}`.
    pub async fn create(
        &self,
        session_id: impl AsRef<str>,
        params: TaskCreateParams,
    ) -> Result<Value> {
        let path = format!("/v1/sessions/{}/tasks", session_id.as_ref());
        self.client.request_json(Method::POST, &path, Some(&params)).await
    }

    /// `PATCH /v1/sessions/{id}/tasks/{task_id}` → partial update.
    pub async fn update(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        params: TaskUpdateParams,
    ) -> Result<Value> {
        let path = format!("/v1/sessions/{}/tasks/{}", session_id.as_ref(), task_id.as_ref());
        self.client.request_json(Method::PATCH, &path, Some(&params)).await
    }

    /// `DELETE /v1/sessions/{id}/tasks/{task_id}` → delete.
    pub async fn delete(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
    ) -> Result<Value> {
        let path = format!("/v1/sessions/{}/tasks/{}", session_id.as_ref(), task_id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }

    /// `POST /v1/sessions/{id}/tasks/{task_id}/move` → move between columns.
    pub async fn move_task(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        status: impl Into<String>,
        sort_order: Option<f64>,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/tasks/{}/move",
            session_id.as_ref(),
            task_id.as_ref()
        );
        let mut body = json!({ "status": status.into() });
        if let Some(o) = sort_order {
            body["sortOrder"] = json!(o);
        }
        self.client.request_json(Method::POST, &path, Some(&body)).await
    }

    /// `PUT /v1/sessions/{id}/tasks/{task_id}/checklist` → replace the checklist.
    pub async fn set_checklist(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        checklist: Value,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/tasks/{}/checklist",
            session_id.as_ref(),
            task_id.as_ref()
        );
        let body = json!({ "checklist": checklist });
        self.client.request_json(Method::PUT, &path, Some(&body)).await
    }

    /// `POST /v1/sessions/{id}/tasks/{task_id}/deps` → add a dependency.
    pub async fn add_dependency(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        blocks_task_id: impl Into<String>,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/tasks/{}/deps",
            session_id.as_ref(),
            task_id.as_ref()
        );
        let body = json!({ "blocksTaskId": blocks_task_id.into() });
        self.client.request_json(Method::POST, &path, Some(&body)).await
    }

    /// `DELETE /v1/sessions/{id}/tasks/{task_id}/deps/{blocks_task_id}` → remove
    /// a dependency.
    pub async fn remove_dependency(
        &self,
        session_id: impl AsRef<str>,
        task_id: impl AsRef<str>,
        blocks_task_id: impl AsRef<str>,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/tasks/{}/deps/{}",
            session_id.as_ref(),
            task_id.as_ref(),
            blocks_task_id.as_ref()
        );
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Projects (session-scoped)
// ════════════════════════════════════════════════════════════════════════════

/// Session-scoped project operations.
#[derive(Clone)]
pub struct Projects {
    client: Client,
}

/// Parameters for creating a project.
#[derive(Debug, Clone, Serialize)]
pub struct ProjectCreateParams {
    /// The project name.
    pub name: String,
    /// An optional description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ProjectCreateParams {
    /// A project from a name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
        }
    }
}

/// Parameters for updating a project.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ProjectUpdateParams {
    /// New name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New description.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Archive flag.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub archived: Option<bool>,
}

impl Projects {
    /// `GET /v1/sessions/{id}/projects` → all projects.
    pub async fn list(&self, session_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/sessions/{}/projects", session_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `POST /v1/sessions/{id}/projects` → create. Returns `{projectId}`.
    pub async fn create(
        &self,
        session_id: impl AsRef<str>,
        params: ProjectCreateParams,
    ) -> Result<Value> {
        let path = format!("/v1/sessions/{}/projects", session_id.as_ref());
        self.client.request_json(Method::POST, &path, Some(&params)).await
    }

    /// `PATCH /v1/sessions/{id}/projects/{project_id}` → update.
    pub async fn update(
        &self,
        session_id: impl AsRef<str>,
        project_id: impl AsRef<str>,
        params: ProjectUpdateParams,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/projects/{}",
            session_id.as_ref(),
            project_id.as_ref()
        );
        self.client.request_json(Method::PATCH, &path, Some(&params)).await
    }

    /// `DELETE /v1/sessions/{id}/projects/{project_id}` → delete.
    pub async fn delete(
        &self,
        session_id: impl AsRef<str>,
        project_id: impl AsRef<str>,
    ) -> Result<Value> {
        let path = format!(
            "/v1/sessions/{}/projects/{}",
            session_id.as_ref(),
            project_id.as_ref()
        );
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Todos (read-only)
// ════════════════════════════════════════════════════════════════════════════

/// Session-scoped todo reads.
#[derive(Clone)]
pub struct Todos {
    client: Client,
}

impl Todos {
    /// `GET /v1/sessions/{id}/todos` → the todo list (read-only).
    pub async fn list(&self, session_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/sessions/{}/todos", session_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Schedules (user-scoped)
// ════════════════════════════════════════════════════════════════════════════

/// User-scoped cron-schedule operations.
#[derive(Clone)]
pub struct Schedules {
    client: Client,
}

/// Parameters for creating a schedule.
#[derive(Debug, Clone, Serialize)]
pub struct ScheduleCreateParams {
    /// The schedule name.
    pub name: String,
    /// The cron expression.
    #[serde(rename = "cronExpr")]
    pub cron_expr: String,
    /// The action kind.
    #[serde(rename = "actionKind")]
    pub action_kind: String,
    /// The action payload.
    #[serde(rename = "actionPayload", skip_serializing_if = "Option::is_none")]
    pub action_payload: Option<Value>,
}

impl ScheduleCreateParams {
    /// A schedule from name + cron + action kind.
    pub fn new(
        name: impl Into<String>,
        cron_expr: impl Into<String>,
        action_kind: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            cron_expr: cron_expr.into(),
            action_kind: action_kind.into(),
            action_payload: None,
        }
    }
    /// Attach the action payload.
    pub fn payload(mut self, payload: Value) -> Self {
        self.action_payload = Some(payload);
        self
    }
}

/// Parameters for updating a schedule.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ScheduleUpdateParams {
    /// Enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// New cron expression.
    #[serde(rename = "cronExpr", skip_serializing_if = "Option::is_none")]
    pub cron_expr: Option<String>,
}

impl Schedules {
    /// `GET /v1/schedules` → all schedules.
    pub async fn list(&self) -> Result<Value> {
        self.client.request_json::<(), _>(Method::GET, "/v1/schedules", None).await
    }

    /// `POST /v1/schedules` → create. Returns `{taskId}`.
    pub async fn create(&self, params: ScheduleCreateParams) -> Result<Value> {
        self.client
            .request_json(Method::POST, "/v1/schedules", Some(&params))
            .await
    }

    /// `PATCH /v1/schedules/{task_id}` → update.
    pub async fn update(
        &self,
        task_id: impl AsRef<str>,
        params: ScheduleUpdateParams,
    ) -> Result<Value> {
        let path = format!("/v1/schedules/{}", task_id.as_ref());
        self.client.request_json(Method::PATCH, &path, Some(&params)).await
    }

    /// `DELETE /v1/schedules/{task_id}` → delete.
    pub async fn delete(&self, task_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/schedules/{}", task_id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Workflows (user-scoped)
// ════════════════════════════════════════════════════════════════════════════

/// User-scoped workflow operations.
#[derive(Clone)]
pub struct Workflows {
    client: Client,
}

/// Parameters for creating a workflow.
#[derive(Debug, Clone, Serialize)]
pub struct WorkflowCreateParams {
    /// The workflow name.
    pub name: String,
    /// The workflow source.
    pub source: String,
}

/// Parameters for updating a workflow.
#[derive(Debug, Clone, Default, Serialize)]
pub struct WorkflowUpdateParams {
    /// New name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// New source.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Enable/disable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

impl Workflows {
    /// `GET /v1/workflows` → all workflows.
    pub async fn list(&self) -> Result<Value> {
        self.client.request_json::<(), _>(Method::GET, "/v1/workflows", None).await
    }

    /// `GET /v1/workflows/{workflow_id}` → one workflow (with source).
    pub async fn retrieve(&self, workflow_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/workflows/{}", workflow_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `POST /v1/workflows` → create. Returns `{workflowId}`.
    pub async fn create(&self, params: WorkflowCreateParams) -> Result<Value> {
        self.client
            .request_json(Method::POST, "/v1/workflows", Some(&params))
            .await
    }

    /// `PATCH /v1/workflows/{workflow_id}` → update.
    pub async fn update(
        &self,
        workflow_id: impl AsRef<str>,
        params: WorkflowUpdateParams,
    ) -> Result<Value> {
        let path = format!("/v1/workflows/{}", workflow_id.as_ref());
        self.client.request_json(Method::PATCH, &path, Some(&params)).await
    }

    /// `DELETE /v1/workflows/{workflow_id}` → delete.
    pub async fn delete(&self, workflow_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/workflows/{}", workflow_id.as_ref());
        self.client.request_json::<(), _>(Method::DELETE, &path, None).await
    }

    /// `POST /v1/workflows/lint` → compile-check a source. Returns `{ok,errors}`.
    pub async fn lint(&self, source: impl Into<String>) -> Result<Value> {
        let body = json!({ "source": source.into() });
        self.client
            .request_json(Method::POST, "/v1/workflows/lint", Some(&body))
            .await
    }

    /// `POST /v1/workflows/{workflow_id}/run` → run. Returns `{runId,status}`.
    pub async fn run(
        &self,
        workflow_id: impl AsRef<str>,
        input: Option<Value>,
        trigger: Option<String>,
    ) -> Result<Value> {
        let path = format!("/v1/workflows/{}/run", workflow_id.as_ref());
        let mut body = json!({});
        if let Some(i) = input {
            body["input"] = i;
        }
        if let Some(t) = trigger {
            body["trigger"] = json!(t);
        }
        self.client.request_json(Method::POST, &path, Some(&body)).await
    }

    /// `GET /v1/workflows/runs/{run_id}` → run status + events + output.
    pub async fn run_logs(&self, run_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/workflows/runs/{}", run_id.as_ref());
        self.client.request_json::<(), _>(Method::GET, &path, None).await
    }

    /// `POST /v1/workflows/runs/{run_id}/cancel` → cancel a run.
    pub async fn cancel_run(&self, run_id: impl AsRef<str>) -> Result<Value> {
        let path = format!("/v1/workflows/runs/{}/cancel", run_id.as_ref());
        self.client.request_json::<(), _>(Method::POST, &path, None).await
    }
}
