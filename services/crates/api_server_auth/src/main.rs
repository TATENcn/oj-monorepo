use api_server_auth::AuthApiServerError;

#[tokio::main]
async fn main() -> Result<(), AuthApiServerError> {
    tracing_subscriber::fmt::init();

    Ok(())
}
