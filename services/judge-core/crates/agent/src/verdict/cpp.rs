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
    sync::mpsc,
    time::timeout,
};

use crate::limit::cgroup::CgroupGuard;
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

    async fn compile(&self, source: &str) -> Result<super::CompileResult, super::VerdictError> {
        let source_path = self.work_dir.join("source.cpp");
        fs::write(&source_path, source).await?;

        let mut cmd = process::Command::new("g++");
        cmd.arg("-std=c++23")
            .arg("-w")
            .arg(source_path.display().to_string())
            .arg("-o")
            .arg("executable")
            .current_dir(&self.work_dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // TODO: Apply resource limits

        let mut child = cmd.spawn()?;
        let result = timeout(Duration::from_secs(10), child.wait()).await;
        match result {
            Ok(status) => {
                let status = status?;
                if status.success() {
                    Ok(super::CompileResult::Success)
                } else {
                    let mut message = String::new();
                    if let Some(mut stderr) = child.stderr.take() {
                        stderr.read_to_string(&mut message).await?;
                    }
                    Ok(super::CompileResult::CompilationError { message })
                }
            }
            Err(_) => {
                child.kill().await?;
                Ok(super::CompileResult::Timeout)
            }
        }
    }

    async fn verdict(
        &self,
        case: shared::models::Case,
        limit: &shared::models::ResourcesLimit,
    ) -> Result<shared::models::VerdictTaskResult, super::VerdictError> {
        let exe_path = self.work_dir.join("executable");

        let mut cmd = process::Command::new(&exe_path);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .current_dir(&self.work_dir);

        let mut child = cmd.spawn()?;

        // Build a unique cgroup id using agent pid + child pid
        let cgroup_id = format!("verdict-{}-{}", std::process::id(), child.id().unwrap_or(0));
        let mut cg = CgroupGuard::new(&cgroup_id, limit)?;
        cg.add_task(child.id().unwrap_or(0) as u64)?;

        // Write stdin input
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(case.input.as_bytes()).await?;
        }

        let output_limit = limit.output_bytes as usize;
        let (output_exceeded_tx, mut output_exceeded_rx) = mpsc::channel::<()>(1);

        let stdout_reader = {
            let stdout = child.stdout.take().unwrap();
            let tx = output_exceeded_tx.clone();
            let mut capped = stdout.take((output_limit + 1) as u64);
            tokio::spawn(async move {
                let mut buf = String::new();
                capped.read_to_string(&mut buf).await?;
                if buf.len() > output_limit {
                    let _ = tx.try_send(());
                }
                io::Result::Ok(buf)
            })
        };
        let stderr_reader = {
            let stderr = child.stderr.take().unwrap();
            let mut capped = stderr.take((output_limit + 1) as u64);
            tokio::spawn(async move {
                let mut buf = String::new();
                capped.read_to_string(&mut buf).await?;
                if buf.len() > output_limit {
                    let _ = output_exceeded_tx.try_send(());
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
                ChildOutcome::Exited(result?)
            }
            _ = output_exceeded_rx.recv() => {
                child.kill().await?;
                let _ = child.wait().await;
                ChildOutcome::OutputExceeded
            }
            _ = tokio::time::sleep(Duration::from_millis(limit.wall_time_ms)) => {
                child.kill().await?;
                let _ = child.wait().await;
                ChildOutcome::WallTimeout
            }
        };

        // Collect stdout and stderr
        let stdout = stdout_reader.await.map_err(io::Error::other)??;
        let stderr = stderr_reader.await.map_err(io::Error::other)??;

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

                    if cg.was_oom_killed() {
                        return Ok(VerdictTaskResult::Killed {
                            reason: KilledReason::MemoryLimitExceeded,
                            stdout,
                            stderr,
                        });
                    }

                    if usage.cpu_time_ms > limit.cpu_time_ms {
                        return Ok(VerdictTaskResult::Killed {
                            reason: KilledReason::CpuTimeLimitExceeded,
                            stdout,
                            stderr,
                        });
                    }

                    if stdout.len() > output_limit || stderr.len() > output_limit {
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
                        Ok(VerdictTaskResult::Accepted { usage })
                    } else {
                        Ok(VerdictTaskResult::WrongAnswer {
                            wrong_case: case,
                            received: stdout,
                            stderr,
                        })
                    }
                } else if let Some(code) = status.code() {
                    Ok(VerdictTaskResult::RuntimeError { stderr, exit_code: code })
                } else {
                    let reason = if cg.was_oom_killed() {
                        KilledReason::MemoryLimitExceeded
                    } else {
                        KilledReason::Signaled {
                            signal: status.signal().unwrap_or(0),
                        }
                    };
                    Ok(VerdictTaskResult::Killed { reason, stdout, stderr })
                }
            }
        }
    }

    async fn cleanup(&self) -> Result<(), super::VerdictError> {
        fs::remove_dir_all(&self.work_dir).await?;

        Ok(())
    }
}
