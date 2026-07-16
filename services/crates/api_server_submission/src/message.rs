use judge_core_shared::models::VerdictTask;
use serde::Serialize;
use uuid::Uuid;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitMessage {
    pub submission_id: Uuid,
    pub task: VerdictTask,
}
