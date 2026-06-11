use std::{
    io,
    os::unix::process::ExitStatusExt,
    path::PathBuf,
    process::{ExitStatus, Stdio},
    time::Duration,
};

use tokio::{
    fs,
    io::{AsyncReadExt, AsyncWriteExt},
    process,
    sync::Notify,
    time::timeout,
};
use tracing::{debug, info, instrument, trace};

use crate::limit::{cgroup::CgroupGuard, seccomp::seccomp_filter};
use crate::truncate_str;
use crate::verdict::Verdict;
use shared::models::{KilledReason, VerdictTaskResult};

pub struct Cpp {
    work_dir: PathBuf,
}

impl Verdict for Cpp {
    async fn prepare(workdir: &std::path::Path, _id: u64) -> Result<Self, super::VerdictError> {
        fs::create_dir_all(workdir).await?;

        Ok(Self {
            work_dir: workdir.to_path_buf(),
        })
    }

    #[instrument(skip(self, source), fields(source_len = source.len()))]
    async fn compile(&self, source: &str) -> Result<super::CompileResult, super::VerdictError> {
        let source_path = self.work_dir.join("source.cpp");

        debug!(source_len = source.len(), "writing source file");
        fs::write(&source_path, source).await?;

        trace!(source = truncate_str(source, 1024), "source content");

        let mut cmd = process::Command::new("ccache");
        cmd.arg("g++")
            .arg("-std=c++23")
            .arg("-w")
            .arg(source_path.display().to_string())
            .arg("-o")
            .arg("executable")
            .current_dir(&self.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // TODO: Apply resource limits

        debug!("spawning g++ process");
        let mut child = cmd.spawn()?;
        let result = timeout(Duration::from_secs(10), child.wait()).await;
        match result {
            Ok(status) => {
                let status = status?;
                if status.success() {
                    debug!("compilation succeeded");
                    Ok(super::CompileResult::Success)
                } else {
                    let mut message = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        stderr.read_to_string(&mut message).await?;
                    }
                    info!(message_len = message.len(), "compilation error");
                    Ok(super::CompileResult::CompilationError { message })
                }
            }
            Err(_) => {
                child.kill().await?;
                info!("compilation timeout");
                Ok(super::CompileResult::Timeout)
            }
        }
    }

    #[instrument(skip(self, case, limit), fields(case_id = id, input_len = case.input.len(), output_limit = limit.output_bytes))]
    async fn verdict(
        &self,
        case: shared::models::Case,
        limit: &shared::models::ResourcesLimit,
        id: u64,
    ) -> Result<shared::models::VerdictTaskResult, super::VerdictError> {
        let exe_path = self.work_dir.join("executable");

        let mut cmd = process::Command::new(&exe_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.work_dir)
            .kill_on_drop(true);

        unsafe {
            cmd.pre_exec(|| {
                let filter = seccomp_filter(shared::models::Language::Cpp).map_err(|e| io::Error::new(io::ErrorKind::Unsupported, e))?;
                filter.load().map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

                Ok(())
            })
        };

        // Build a unique cgroup id using verdict id
        // FIXME: there's a race condition when the child is created and when it's added to the cgroup, but I don't know how to fix it gracefully
        let cgroup_id = format!("verdict-cpp-{}", id);
        let mut cg = CgroupGuard::new(&cgroup_id, limit)?;
        debug!(cgroup_id = %cgroup_id, "cgroup created");

        let mut child = cmd.spawn()?;
        let child_pid = child.id().unwrap_or(0);
        debug!(child_pid, "child process spawned");

        cg.add_task(child_pid as u64)?;

        // Write stdin input
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(case.input.as_bytes()).await?;
        }
        trace!(input = truncate_str(&case.input, 1024), "stdin written");

        let output_limit = limit.output_bytes as usize;
        let output_exceeded = std::sync::Arc::new(Notify::new());

        let stdout_reader = {
            let stdout = child.stdout.take().unwrap();
            let output_exceeded = output_exceeded.clone();
            let mut capped = stdout.take((output_limit + 1) as u64);
            tokio::spawn(async move {
                let mut buf = String::new();
                capped.read_to_string(&mut buf).await?;
                if buf.len() > output_limit {
                    output_exceeded.notify_one();
                }
                io::Result::Ok(buf)
            })
        };
        let stderr_reader = {
            let stderr = child.stderr.take().unwrap();
            let output_exceeded = output_exceeded.clone();
            let mut capped = stderr.take((output_limit + 1) as u64);
            tokio::spawn(async move {
                let mut buf = String::new();
                capped.read_to_string(&mut buf).await?;
                if buf.len() > output_limit {
                    output_exceeded.notify_one();
                }
                io::Result::Ok(buf)
            })
        };

        enum ChildOutcome {
            Exited(ExitStatus),
            OutputExceeded,
            WallTimeout,
        }

        let outcome = tokio::select! {
            result = child.wait() => {
                let status = result?;
                debug!(status = ?status, "child exited normally");
                ChildOutcome::Exited(status)
            }
            _ = output_exceeded.notified() => {
                child.kill().await?;
                let _ = child.wait().await;
                info!("output limit exceeded, killed");
                ChildOutcome::OutputExceeded
            }
            _ = tokio::time::sleep(Duration::from_millis(limit.wall_time_ms)) => {
                child.kill().await?;
                let _ = child.wait().await;
                info!("wall time limit exceeded, killed");
                ChildOutcome::WallTimeout
            }
        };

        // Collect stdout and stderr
        let stdout = stdout_reader.await.map_err(io::Error::other)??;
        let stderr = stderr_reader.await.map_err(io::Error::other)??;
        trace!(stdout_len = stdout.len(), stderr_len = stderr.len(), "collected output");
        trace!(stdout = truncate_str(&stdout, 1024), stderr = truncate_str(&stderr, 1024), "output content");

        match outcome {
            ChildOutcome::OutputExceeded => Ok(VerdictTaskResult::Killed {
                reason: KilledReason::OutputLimitExceeded,
                stdout,
                stderr,
            }),
            ChildOutcome::WallTimeout => Ok(VerdictTaskResult::Killed {
                reason: KilledReason::WallTimeLimitExceeded,
                stdout,
                stderr,
            }),
            ChildOutcome::Exited(status) => {
                if status.success() {
                    let usage = cg.usage();
                    debug!(?usage, "resource usage collected");

                    if usage.cpu_time_ms > limit.cpu_time_ms {
                        info!(?usage, cpu_limit = limit.cpu_time_ms, "cpu time limit exceeded");
                        return Ok(VerdictTaskResult::Killed {
                            reason: KilledReason::CpuTimeLimitExceeded,
                            stdout,
                            stderr,
                        });
                    }

                    if stdout.len() > output_limit || stderr.len() > output_limit {
                        info!(stdout_len = stdout.len(), stderr_len = stderr.len(), output_limit, "output limit exceeded");
                        return Ok(VerdictTaskResult::Killed {
                            reason: KilledReason::OutputLimitExceeded,
                            stdout,
                            stderr,
                        });
                    }

                    // Exact output comparison
                    let expected = case.output.as_str();
                    let received = stdout.as_str();

                    if expected == received {
                        info!(?usage, "accepted");
                        Ok(VerdictTaskResult::Accepted { usage })
                    } else {
                        info!(expected_len = expected.len(), received_len = received.len(), "wrong answer");
                        Ok(VerdictTaskResult::WrongAnswer {
                            wrong_case: case,
                            received: stdout,
                            stderr,
                        })
                    }
                } else if let Some(code) = status.code() {
                    info!(exit_code = code, "runtime error");
                    Ok(VerdictTaskResult::RuntimeError { stderr, exit_code: code })
                } else {
                    let reason = if cg.was_oom_killed() {
                        info!("oom killed");
                        KilledReason::MemoryLimitExceeded
                    } else {
                        let signal = status.signal().unwrap_or(0);
                        info!(signal, "signaled");
                        KilledReason::Signaled { signal }
                    };
                    Ok(VerdictTaskResult::Killed { reason, stdout, stderr })
                }
            }
        }
    }

    async fn cleanup(&self) -> Result<(), super::VerdictError> {
        fs::remove_dir_all(&self.work_dir).await?;
        debug!(work_dir = %self.work_dir.display(), "cleaned up workdir");
        Ok(())
    }
}
