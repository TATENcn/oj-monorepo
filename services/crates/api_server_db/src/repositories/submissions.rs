use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, EntityTrait, QueryFilter};
use uuid::Uuid;

use crate::{
    models::{
        enums::{AcceptableLanguage, SubmissionStatus},
        http::SubmissionResult,
        problems::{self, Entity as ProblemEntity},
        submissions::{self, Entity as SubmissionEntity},
        test_cases,
    },
    repo_struct,
};
use judge_core_shared::models::http::VerdictResponse;
use judge_core_shared::models::{Case, Language, ResourcesLimit, VerdictTask};

use super::RepoError;

repo_struct!(SubmissionRepo);

impl SubmissionRepo {
    pub async fn create_pending(
        &self,
        problem_id: Uuid,
        user_id: Uuid,
        source_code: String,
        language: AcceptableLanguage,
    ) -> Result<(Uuid, VerdictTask), RepoError> {
        let problem = ProblemEntity::find_by_id(problem_id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        let cases: Vec<Case> = test_cases::Entity::find()
            .filter(test_cases::Column::ProblemId.eq(problem_id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|m| Case {
                input: m.input,
                output: m.output,
            })
            .collect();

        let submission_id = Uuid::now_v7();
        let now = Utc::now();

        let submission = submissions::ActiveModel {
            id: Set(submission_id),
            problem_id: Set(problem_id),
            user_id: Set(user_id),
            source_code: Set(source_code.clone()),
            status: Set(SubmissionStatus::Pending),
            result: Set(None),
            language: Set(language),
            submitted_at: Set(now),
            completed_at: Set(None),
        };
        submission.insert(&self.db).await?;

        let verdict_task = VerdictTask {
            source: source_code,
            language: language.into(),
            cases,
            limits: ResourcesLimit {
                cpu_time_ms: problem.limit_cpu_time_ms,
                wall_time_ms: problem.limit_wall_time_ms,
                memory_bytes: problem.limit_memory_bytes,
                output_bytes: problem.limit_output_bytes,
            },
            stop_on_first_error: true,
        };

        Ok((submission_id, verdict_task))
    }

    pub async fn mark_completed(&self, submission_id: Uuid, result: &VerdictResponse) -> Result<(), RepoError> {
        let submission = SubmissionEntity::find_by_id(submission_id).one(&self.db).await?.ok_or(RepoError::NotFound)?;
        let result_json = serde_json::to_value(result).map_err(|e| RepoError::Internal(sea_orm::DbErr::Custom(e.to_string())))?;

        let mut active: submissions::ActiveModel = submission.into();
        active.status = Set(SubmissionStatus::Completed);
        active.result = Set(Some(result_json));
        active.completed_at = Set(Some(Utc::now()));
        active.update(&self.db).await?;

        Ok(())
    }

    /// Get a submission's result
    ///
    /// ### Returns
    /// - [`None`] if the submission doesn't exist
    /// - [`Some(Ok("pending"))`] if still pending
    /// - [`Some(Ok(VerdictResponse))`] if completed
    pub async fn get(&self, id: Uuid) -> Result<Option<SubmissionResult>, RepoError> {
        let submission = SubmissionEntity::find_by_id(id).one(&self.db).await?;

        let Some(submission) = submission else {
            return Ok(None);
        };

        if submission.status == SubmissionStatus::Pending {
            return Ok(Some(SubmissionResult::Pending));
        }

        let json = submission
            .result
            .ok_or(RepoError::Internal(sea_orm::DbErr::Custom("missing result for completed submission".into())))?;
        let verdict: VerdictResponse = serde_json::from_value(json).map_err(|e| RepoError::Internal(sea_orm::DbErr::Custom(e.to_string())))?;

        Ok(Some(SubmissionResult::Completed(verdict)))
    }
}

impl From<AcceptableLanguage> for Language {
    fn from(lang: AcceptableLanguage) -> Self {
        match lang {
            AcceptableLanguage::Cpp => Language::Cpp,
        }
    }
}
