use chrono::Utc;
use sea_orm::{
    ActiveModelTrait,
    ActiveValue::Set,
    ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, TransactionTrait,
    sea_query::{BinOper, Expr},
};
use uuid::Uuid;

use crate::{
    models::{
        http,
        problems::{self, Entity as ProblemEntity},
        problems_tags,
    },
    repo_struct,
};

use super::RepoError;

repo_struct!(ProblemsRepo);

impl ProblemsRepo {
    pub async fn list(&self, queries: http::ListProblemQueries) -> Result<http::ListProblemResponse, RepoError> {
        let limit = queries.limit.unwrap_or(20).min(100);
        let page = queries.page.unwrap_or(0);

        // Tags AND filter
        let tag_filtered_ids: Option<Vec<Uuid>> = if let Some(ref tag_ids) = queries.tag {
            match tag_ids.is_empty() {
                true => None,
                false => {
                    let problem_ids = problems_tags::Entity::find()
                        .select_only()
                        .column(problems_tags::Column::ProblemId)
                        .filter(problems_tags::Column::TagId.is_in(tag_ids.clone()))
                        .group_by(problems_tags::Column::ProblemId)
                        .having(sea_orm::ExprTrait::binary(
                            sea_orm::ExprTrait::count(Expr::col(problems_tags::Column::TagId)),
                            BinOper::Equal,
                            Expr::val(tag_ids.len() as i64),
                        ))
                        .into_tuple::<Uuid>()
                        .all(&self.db)
                        .await?;
                    Some(problem_ids)
                }
            }
        } else {
            None
        };

        // Early returns
        if let Some(ref ids) = tag_filtered_ids {
            if ids.is_empty() {
                return Ok(http::ListProblemResponse { problems: vec![], total: 0 });
            }
        }

        // Build main problem query
        let mut select = ProblemEntity::find().filter(problems::Column::DeletedAt.is_null());

        if let Some(ref query_str) = queries.query {
            select = select.filter(problems::Column::Title.contains(query_str));
        }
        if let Some(difficulty) = queries.difficulty {
            select = select.filter(problems::Column::Difficulty.eq(difficulty));
        }
        if let Some(ref ids) = tag_filtered_ids {
            select = select.filter(problems::Column::Id.is_in(ids.clone()));
        }

        // Count total matching
        let total = select.clone().count(&self.db).await?;

        // Fetch page
        let problem_models = select
            .order_by_asc(problems::Column::CreatedAt)
            .offset(page * limit)
            .limit(limit)
            .all(&self.db)
            .await?;

        // Fetch tags for the returned problems
        let problem_ids: Vec<Uuid> = problem_models.iter().map(|p| p.id).collect();
        let tag_rows = match problem_ids.is_empty() {
            true => vec![],
            false => {
                problems_tags::Entity::find()
                    .filter(problems_tags::Column::ProblemId.is_in(problem_ids))
                    .all(&self.db)
                    .await?
            }
        };

        // Assemble response
        let problems: Vec<http::ListProblemResponseItem> = problem_models
            .into_iter()
            .map(|p| {
                let problem_tags: Vec<Uuid> = tag_rows.iter().filter(|pt| pt.problem_id == p.id).map(|pt| pt.tag_id).collect();
                http::ListProblemResponseItem {
                    id: p.id,
                    title: p.title,
                    difficulty: p.difficulty,
                    tags: problem_tags,
                }
            })
            .collect();

        Ok(http::ListProblemResponse { problems, total })
    }

    pub async fn create(&self, req: http::CreateProblemRequest, author_id: Uuid) -> Result<http::CreateProblemResponse, RepoError> {
        let problem_id = Uuid::now_v7();
        let now = Utc::now();

        let txn = self.db.begin().await?;

        let problem = problems::ActiveModel {
            id: Set(problem_id),
            title: Set(req.title),
            description: Set(req.description),
            difficulty: Set(req.difficulty),
            author_id: Set(author_id),
            created_at: Set(now),
            updated_at: Set(now),
            deleted_at: Set(None),
            limit_cpu_time_ms: Set(req.limit.cpu_time_ms),
            limit_wall_time_ms: Set(req.limit.wall_time_ms),
            limit_memory_bytes: Set(req.limit.memory_bytes),
            limit_output_bytes: Set(req.limit.output_bytes),
        };
        problem.insert(&txn).await?;

        for tag_id in &req.tags {
            problems_tags::ActiveModel::builder()
                .set_problem_id(problem_id)
                .set_tag_id(*tag_id)
                .insert(&txn)
                .await?;
        }

        txn.commit().await?;

        Ok(http::CreateProblemResponse { id: problem_id })
    }

    pub async fn get_stat(&self) -> Result<http::ProblemStatResponse, RepoError> {
        let total = ProblemEntity::find().filter(problems::Column::DeletedAt.is_null()).count(&self.db).await?;
        Ok(http::ProblemStatResponse { total })
    }

    pub async fn get_by_id(&self, id: Uuid) -> Result<http::ProblemDetailResponse, RepoError> {
        let problem = ProblemEntity::find_by_id(id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        let tags: Vec<Uuid> = problems_tags::Entity::find()
            .filter(problems_tags::Column::ProblemId.eq(id))
            .all(&self.db)
            .await?
            .into_iter()
            .map(|pt| pt.tag_id)
            .collect();

        Ok(http::ProblemDetailResponse {
            id: problem.id,
            author_id: problem.author_id,
            title: problem.title,
            description: problem.description,
            difficulty: problem.difficulty,
            tags,
            created_at: problem.created_at,
            updated_at: problem.updated_at,
            limit: http::ProblemLimit {
                cpu_time_ms: problem.limit_cpu_time_ms,
                wall_time_ms: problem.limit_wall_time_ms,
                memory_bytes: problem.limit_memory_bytes,
                output_bytes: problem.limit_output_bytes,
            },
        })
    }

    pub async fn update(&self, id: Uuid, author_id: Uuid, req: http::UpdateProblemRequest) -> Result<(), RepoError> {
        let problem = ProblemEntity::find_by_id(id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        if problem.author_id != author_id {
            return Err(RepoError::Forbidden);
        }

        let mut active: problems::ActiveModel = problem.into();
        active.updated_at = Set(Utc::now());

        if let Some(title) = req.title {
            active.title = Set(title);
        }
        if let Some(description) = req.description {
            active.description = Set(description);
        }
        if let Some(difficulty) = req.difficulty {
            active.difficulty = Set(difficulty);
        }
        if let Some(limit) = req.limit {
            if let Some(cpu_time_ms) = limit.cpu_time_ms {
                active.limit_cpu_time_ms = Set(cpu_time_ms);
            }
            if let Some(wall_time_ms) = limit.wall_time_ms {
                active.limit_wall_time_ms = Set(wall_time_ms);
            }
            if let Some(memory_bytes) = limit.memory_bytes {
                active.limit_memory_bytes = Set(memory_bytes);
            }
            if let Some(output_bytes) = limit.output_bytes {
                active.limit_output_bytes = Set(output_bytes);
            }
        }

        let txn = self.db.begin().await?;

        if let Some(tags) = req.tags {
            problems_tags::Entity::delete_many()
                .filter(problems_tags::Column::ProblemId.eq(id))
                .exec(&txn)
                .await?;

            for tag_id in &tags {
                problems_tags::ActiveModel::builder()
                    .set_problem_id(id)
                    .set_tag_id(*tag_id)
                    .insert(&txn)
                    .await?;
            }
        }

        active.update(&txn).await?;
        txn.commit().await?;

        Ok(())
    }

    pub async fn delete(&self, id: Uuid, author_id: Uuid) -> Result<(), RepoError> {
        let problem = ProblemEntity::find_by_id(id)
            .filter(problems::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?
            .ok_or(RepoError::NotFound)?;

        if problem.author_id != author_id {
            return Err(RepoError::Forbidden);
        }

        let mut active: problems::ActiveModel = problem.into();
        active.deleted_at = Set(Some(Utc::now()));
        active.update(&self.db).await?;

        Ok(())
    }
}
