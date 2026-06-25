use chrono::Utc;
use sea_orm::{
    ActiveValue::{NotSet, Set},
    entity::prelude::*,
};

#[sea_orm::model]
#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "users")]
pub struct Model {
    /// User id
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: uuid::Uuid,
    /// User email (lowercased)
    #[sea_orm(unique)]
    pub email: String,
    /// Whether user email is verified
    pub email_verified: bool,
    /// User password hash
    pub password: String,
    /// User created at
    pub created_at: chrono::DateTime<Utc>,
    /// User updated at
    pub updated_at: chrono::DateTime<Utc>,
}

#[async_trait::async_trait]
impl ActiveModelBehavior for ActiveModel {
    fn new() -> Self {
        let now: chrono::DateTime<Utc> = Utc::now();
        Self {
            id: Set(uuid::Uuid::now_v7()),
            email: NotSet,
            email_verified: Set(false),
            password: NotSet,
            created_at: Set(now),
            updated_at: Set(now),
        }
    }

    async fn before_save<C>(mut self, _db: &C, insert: bool) -> Result<Self, DbErr>
    where
        C: ConnectionTrait,
    {
        if !insert {
            self.updated_at = Set(Utc::now());
        }

        Ok(self)
    }
}
