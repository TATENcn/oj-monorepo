use shared::models::http::{
    ACCEPTABLE_URL, ERR_AGENT_BUSY, ERR_AGENT_UNAVAILABLE, ERR_CONNECTION_FAILED, ERR_MAX_RETRIES_EXCEEDED, ERR_PROTOCOL_ERROR, ERR_PROVISION_ERROR,
    ERR_QUEUE_FULL, ERR_SHUTTING_DOWN, ERR_TASK_TIMEOUT, ErrorResponse, METRICS_URL, SuccessResponse, TASK_URL, VerdictResponse,
};

pub use shared::models::{
    VerdictTask, VerdictTaskResult,
    http::{AcceptablezResponse, PoolMetrics},
};

#[derive(Debug)]
pub struct JudgeCoreClient {
    standalone_mode: bool,
    base_url: String,
    client: reqwest::Client,
}

impl JudgeCoreClient {
    pub fn new(base_url: impl Into<String>, standalone_mode: bool) -> JudgeCoreClient {
        Self {
            standalone_mode,
            base_url: base_url.into(),
            client: reqwest::Client::new(),
        }
    }

    pub async fn metrics(&self) -> Result<PoolMetrics, JudgeCoreError> {
        self.assert_not_standalone()?;
        let response = self.client.get(self.merge_url(METRICS_URL)).send().await?;
        Self::parse_success(response).await
    }

    pub async fn acceptable(&self) -> Result<AcceptablezResponse, JudgeCoreError> {
        self.assert_not_standalone()?;
        let response = self.client.get(self.merge_url(ACCEPTABLE_URL)).send().await?;
        Self::parse_success(response).await
    }

    pub async fn task_submit(&self, task: &VerdictTask) -> Result<VerdictTaskResult, JudgeCoreError> {
        let response = self.client.post(self.merge_url(TASK_URL)).json(task).send().await?;
        let verdict: VerdictResponse = Self::parse_success(response).await?;
        Ok(verdict.into())
    }

    pub fn is_standalone(&self) -> &bool {
        &self.standalone_mode
    }

    fn assert_not_standalone(&self) -> Result<(), JudgeCoreError> {
        match self.standalone_mode {
            true => Err(JudgeCoreError::UsingStandaloneMode),
            false => Ok(()),
        }
    }

    fn merge_url(&self, url: &str) -> String {
        format!("{}{}", self.base_url, url)
    }

    async fn parse_success<T: serde::de::DeserializeOwned>(response: reqwest::Response) -> Result<T, JudgeCoreError> {
        match response.status().is_success() {
            true => Ok(response.json::<SuccessResponse<T>>().await?.data),
            false => Err(response.json::<ErrorResponse>().await?.into()),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JudgeCoreError {
    #[error("using standalone mode")]
    UsingStandaloneMode,
    #[error("queue full")]
    QueueFull,
    #[error("max retries exceeded")]
    MaxRetriesExceeded,
    #[error("no agent available")]
    AgentUnavailable,
    #[error("shutting down")]
    ShuttingDown,
    #[error("task timeout")]
    TaskTimeout,
    #[error("connection failed")]
    ConnectionFailed,
    #[error("protocol error")]
    ProtocolError,
    #[error("provision error")]
    ProvisionError,
    #[error("agent busy")]
    AgentBusy,
    #[error(transparent)]
    Request(#[from] reqwest::Error),
}

impl From<ErrorResponse> for JudgeCoreError {
    fn from(value: ErrorResponse) -> Self {
        match value.error.code.as_str() {
            ERR_QUEUE_FULL => Self::QueueFull,
            ERR_MAX_RETRIES_EXCEEDED => Self::MaxRetriesExceeded,
            ERR_AGENT_UNAVAILABLE => Self::AgentUnavailable,
            ERR_SHUTTING_DOWN => Self::ShuttingDown,
            ERR_TASK_TIMEOUT => Self::TaskTimeout,
            ERR_CONNECTION_FAILED => Self::ConnectionFailed,
            ERR_PROTOCOL_ERROR => Self::ProtocolError,
            ERR_PROVISION_ERROR => Self::ProvisionError,
            ERR_AGENT_BUSY => Self::AgentBusy,
            code => panic!("unexpected server error code: {code}"),
        }
    }
}
