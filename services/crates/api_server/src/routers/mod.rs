use api_server_db::repositories::{problems::ProblemsRepo, test_cases::TestCasesRepo};
use sea_orm::DatabaseConnection;

pub mod problems;

pub struct ApiServerState {
    pub db: DatabaseConnection,
    pub problems_repo: ProblemsRepo,
    pub test_cases_repo: TestCasesRepo,
}
