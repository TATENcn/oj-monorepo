use chrono::{DateTime, Utc};
use sea_orm::entity::prelude::*;

use crate::models::enums::{AcceptableLanguage, SubmissionStatus};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "submissions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: uuid::Uuid,
    pub problem_id: uuid::Uuid,
    pub user_id: uuid::Uuid,

    pub source_code: String,
    pub status: SubmissionStatus,
    pub result: Option<serde_json::Value>,
    pub language: AcceptableLanguage,

    pub submitted_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,

    #[sea_orm(belongs_to, from = "problem_id", to = "id")]
    pub problem: HasOne<super::problems::Entity>,
    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<auth::models::users::Entity>,
}

impl ActiveModelBehavior for ActiveModel {}
