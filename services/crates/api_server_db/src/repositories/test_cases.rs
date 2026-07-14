use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, TransactionTrait};
use uuid::Uuid;

use crate::{
    models::{
        enums, http,
        problems::{self, Entity as ProblemEntity},
        test_cases,
    },
    repo_struct,
};

use super::RepoError;

repo_struct!(TestCasesRepo);

impl TestCasesRepo {
    pub async fn get_by_problem_id(&self, problem_id: Uuid, user_id: Uuid) -> Result<Vec<http::TestCaseSuccessfulResponse>, RepoError> {
        let problem = ProblemEntity::find_by_id(problem_id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        let is_author = problem.author_id == user_id;

        let mut select = test_cases::Entity::find().filter(test_cases::Column::ProblemId.eq(problem_id));
        if !is_author {
            select = select.filter(test_cases::Column::CaseType.eq(enums::CaseType::Example));
        }
        let cases = select.all(&self.db).await?;

        Ok(cases
            .into_iter()
            .map(|c| http::TestCaseSuccessfulResponse {
                id: c.id,
                input: c.input,
                output: c.output,
                case_type: c.case_type,
            })
            .collect())
    }

    pub async fn replace(&self, problem_id: Uuid, req: http::ReplaceTestCasesRequest) -> Result<(), RepoError> {
        // Verify problem exists
        ProblemEntity::find_by_id(problem_id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        let txn = self.db.begin().await?;

        test_cases::Entity::delete_many()
            .filter(test_cases::Column::ProblemId.eq(problem_id))
            .exec(&txn)
            .await?;

        for case in req.cases {
            let tc = test_cases::ActiveModel::builder()
                .set_id(Uuid::now_v7())
                .set_problem_id(problem_id)
                .set_input(case.input)
                .set_output(case.output)
                .set_case_type(case.case_type);
            tc.insert(&txn).await?;
        }

        txn.commit().await?;
        Ok(())
    }
}
