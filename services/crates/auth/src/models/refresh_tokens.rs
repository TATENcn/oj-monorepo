use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    entity::prelude::*,
};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "refresh_tokens")]
pub struct Model {
    /// Refresh token id
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: uuid::Uuid,
    /// User id
    pub user_id: Uuid,
    /// Refresh token hash
    pub token: String,
    /// Refresh token created at
    pub created_at: chrono::DateTime<Utc>,
    /// Refresh token expired at
    pub expired_at: chrono::DateTime<Utc>,

    #[sea_orm(belongs_to, from = "user_id", to = "id")]
    pub user: HasOne<super::users::Entity>,
}

impl ActiveModelBehavior for ActiveModel {
    fn new() -> Self {
        Self {
            id: Set(uuid::Uuid::now_v7()),
            user_id: NotSet,
            token: NotSet,
            created_at: Set(Utc::now()),
            expired_at: NotSet,
        }
    }
}
