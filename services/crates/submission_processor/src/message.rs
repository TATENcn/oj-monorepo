use judge_core_shared::models::VerdictTask;
use serde::Deserialize;

/// Message received from the submit queue — mirrors `packages/models/src/message.ts`.
#[derive(Debug, Deserialize)]
pub struct SubmitMessage {
    pub submission_id: String,
    pub task: VerdictTask,
}
