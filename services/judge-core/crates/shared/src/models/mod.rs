use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResourcesLimit {
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub memory_bytes: u64,
    pub output_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ResourcesUsage {
    pub cpu_time_ms: u64,
    pub wall_time_ms: u64,
    pub memory_bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Case {
    pub input: String,
    pub output: String,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct VerdictTask {
    pub source: String,
    pub cases: Vec<Case>,
    pub limits: ResourcesLimit,
    pub language: Language,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum KilledReason {
    MemoryLimitExceeded,
    WallTimeLimitExceeded,
    CpuTimeLimitExceeded,
    OutputLimitExceeded,
    Signaled { signal: i32 },
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub enum VerdictTaskResult {
    CompilationError {
        message: String,
    },
    Accepted {
        usage: ResourcesUsage, // Maximum
    },
    Killed {
        reason: KilledReason,
        stdout: String,
        stderr: String,
    },
    WrongAnswer {
        wrong_case: Case,
        received: String,
        stderr: String,
    },
    Internal {
        message: String,
    },
    RuntimeError {
        stderr: String,
        exit_code: i32,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, Copy, Eq, PartialEq)]
pub enum Language {
    Cpp,
}
