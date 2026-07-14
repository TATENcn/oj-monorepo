use sea_orm::{ConnectOptions, Database, DatabaseConnection, DbErr};

pub mod problems;
pub mod test_cases;

#[macro_export]
macro_rules! repo_struct {
    ($name:ident) => {
        pub struct $name {
            db: ::sea_orm::DatabaseConnection,
        }

        impl crate::repositories::Repo for $name {
            fn init(connection: ::sea_orm::DatabaseConnection) -> Self {
                Self { db: connection }
            }
        }
    };
}

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error(transparent)]
    Internal(#[from] sea_orm::DbErr),
}

pub trait Repo {
    fn init(connection: DatabaseConnection) -> Self;
}

pub fn connect_repo<T: Repo>(connection: DatabaseConnection) -> T {
    Repo::init(connection)
}

pub async fn connect_db(opt: impl Into<ConnectOptions>) -> Result<DatabaseConnection, DbErr> {
    Database::connect(opt).await
}
