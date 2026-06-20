use serde::{Deserialize, Serialize};

use super::{Case, KilledReason, ResourcesUsage, VerdictTaskResult};

pub const METRICS_URL: &str = "/metricsz";
pub const ACCEPTABLE_URL: &str = "/acceptablez";
pub const TASK_URL: &str = "/task";

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "status")]
pub enum VerdictResponse {
    #[serde(rename = "compilation_error")]
    CompilationError { message: String },
    #[serde(rename = "accepted")]
    Accepted { usage: ResourcesUsage },
    #[serde(rename = "killed")]
    Killed { reason: KilledReason, stdout: String, stderr: String },
    #[serde(rename = "wrong_answer")]
    WrongAnswer { wrong_case: Case, received: String, stderr: String },
    #[serde(rename = "internal")]
    Internal { message: String },
    #[serde(rename = "runtime_error")]
    RuntimeError { stderr: String, exit_code: i32 },
}

impl From<VerdictTaskResult> for VerdictResponse {
    fn from(v: VerdictTaskResult) -> Self {
        match v {
            VerdictTaskResult::CompilationError { message } => VerdictResponse::CompilationError { message },
            VerdictTaskResult::Accepted { usage } => VerdictResponse::Accepted { usage },
            VerdictTaskResult::Killed { reason, stdout, stderr } => VerdictResponse::Killed { reason, stdout, stderr },
            VerdictTaskResult::WrongAnswer { wrong_case, received, stderr } => VerdictResponse::WrongAnswer { wrong_case, received, stderr },
            VerdictTaskResult::Internal { message } => VerdictResponse::Internal { message },
            VerdictTaskResult::RuntimeError { stderr, exit_code } => VerdictResponse::RuntimeError { stderr, exit_code },
        }
    }
}

#[derive(Serialize)]
pub struct ErrorBody {
    pub code: &'static str,
}

#[derive(Serialize)]
pub struct ErrorResponse {
    pub error: ErrorBody,
    pub message: String,
}

#[derive(Serialize)]
pub struct SuccessResponse<T: Serialize> {
    pub data: T,
    pub message: &'static str,
}

#[derive(Serialize, Deserialize)]
pub struct AcceptablezResponse {
    pub acceptable: bool,
    pub metrics: PoolMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    pub queue_size: usize,
    pub agent_count: usize,
    pub healthy_agent_count: usize,
    pub active_tasks: u32,
    pub draining_agent_count: usize,
    pub unhealthy_agent_count: usize,
}
