pub mod problems;
pub mod test_cases;

#[macro_export]
macro_rules! repo_struct {
    ($name:ident) => {
        pub struct $name {
            db: ::sea_orm::DatabaseConnection,
        }

        impl $name {
            pub fn new(connection: ::sea_orm::DatabaseConnection) -> Self {
                Self { db: connection }
            }
        }
    };
}

use sea_orm::DbErr;

#[derive(Debug, thiserror::Error)]
pub enum RepoError {
    #[error("not found")]
    NotFound,
    #[error("forbidden")]
    Forbidden,
    #[error(transparent)]
    Internal(#[from] DbErr),
}
