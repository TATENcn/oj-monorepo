pub mod cpp;

use std::path::Path;
use std::sync::Arc;

use futures::StreamExt;
use futures::stream::FuturesUnordered;
use shared::models::{Case, ResourcesLimit, ResourcesUsage, VerdictTask, VerdictTaskResult};
use tokio::io;
use tokio_util::sync::CancellationToken;

pub enum CompileResult {
    Success,
    Timeout,
    CompilationError { message: String },
}

pub trait Verdict: Sized + Send + Sync {
    /// Create workdir, setting up environments
    fn prepare(workdir: &Path, id: u64) -> impl std::future::Future<Output = Result<Self, VerdictError>> + Send;

    /// Compile source code to executable
    fn compile(&self, source: &str) -> impl std::future::Future<Output = Result<CompileResult, VerdictError>> + Send;

    /// Verdict test case
    fn verdict(&self, case: Case, limit: &ResourcesLimit) -> impl std::future::Future<Output = Result<VerdictTaskResult, VerdictError>> + Send;

    /// Cleanup workdir and environments
    fn cleanup(&self) -> impl std::future::Future<Output = Result<(), VerdictError>> + Send;
}

#[derive(Debug, thiserror::Error)]
pub enum VerdictError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("cancelled")]
    Cancelled,
    #[error(transparent)]
    Cgroup(#[from] cgroups_rs::fs::error::Error),
}

pub async fn handle<T: Verdict + 'static>(id: u64, task: VerdictTask) -> VerdictTaskResult {
    let workdir = Path::new("/work").join(id.to_string());

    let judge = match T::prepare(&workdir, id).await {
        Ok(judge) => judge,
        Err(e) => {
            return VerdictTaskResult::Internal { message: e.to_string() };
        }
    };

    let compile_result = match judge.compile(&task.source).await {
        Ok(result) => result,
        Err(e) => {
            let _ = judge.cleanup().await;
            return VerdictTaskResult::Internal { message: e.to_string() };
        }
    };

    match compile_result {
        CompileResult::CompilationError { message } => {
            let _ = judge.cleanup().await;
            return VerdictTaskResult::CompilationError { message };
        }
        CompileResult::Timeout => {
            let _ = judge.cleanup().await;
            return VerdictTaskResult::CompilationError { message: "Timeout".into() };
        }
        CompileResult::Success => {}
    }

    let judge = Arc::new(judge);
    let cancel = CancellationToken::new();
    let mut futures = FuturesUnordered::new();

    for case in task.cases {
        let j = Arc::clone(&judge);
        let c = cancel.clone();
        let limits = task.limits.clone();

        futures.push(tokio::spawn(async move {
            tokio::select! {
                biased;
                _ = c.cancelled() => Err(VerdictError::Cancelled),
                res = j.verdict(case, &limits) => res,
            }
        }));
    }

    let mut max_usage = ResourcesUsage {
        cpu_time_ms: 0,
        wall_time_ms: 0,
        memory_bytes: 0,
    };

    while let Some(result) = futures.next().await {
        let result = match result {
            Ok(r) => r,
            Err(e) => {
                cancel.cancel();
                drop(futures);
                let _ = judge.cleanup().await;
                return VerdictTaskResult::Internal { message: e.to_string() };
            }
        };

        let verdict = match result {
            Ok(v) => v,
            Err(VerdictError::Cancelled) => continue,
            Err(e) => {
                cancel.cancel();
                drop(futures);
                let _ = judge.cleanup().await;
                return VerdictTaskResult::Internal { message: e.to_string() };
            }
        };

        match verdict {
            VerdictTaskResult::Accepted { usage } => {
                max_usage.cpu_time_ms = max_usage.cpu_time_ms.max(usage.cpu_time_ms);
                max_usage.wall_time_ms = max_usage.wall_time_ms.max(usage.wall_time_ms);
                max_usage.memory_bytes = max_usage.memory_bytes.max(usage.memory_bytes);
            }
            other => {
                cancel.cancel();
                drop(futures);
                let _ = judge.cleanup().await;
                return other;
            }
        }
    }

    let _ = judge.cleanup().await;

    VerdictTaskResult::Accepted { usage: max_usage }
}
