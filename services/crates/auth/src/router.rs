use std::sync::Arc;

use axum::{
    Form, Json, Router,
    extract::State,
    http::StatusCode,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use sha2::{Digest, Sha256};
use tracing::{error, info, warn};

use crate::{
    hash,
    models::{
        http::{
            AccessTokenType, Jwk, JwksResponse, RegisterErrorResponse, RegisterRequest, RegisterResponse, TokenIntrospectionRequest,
            TokenIntrospectionResponse, TokenOperationErrorResponse, TokenRequest, TokenResponse, TokenRevocationRequest,
        },
        refresh_tokens, users,
    },
    token::{self, TokenType},
};

pub struct AppState {
    pub db: DatabaseConnection,
    pub private_key_pem: Vec<u8>,
    pub public_key_pem: Vec<u8>,
    pub access_token_ttl_secs: u64,
    pub refresh_token_ttl_secs: u64,
}

type HandlerError = (StatusCode, Json<TokenOperationErrorResponse>);

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        // POST `/token` [RFC 6749#2.3.1](https://datatracker.ietf.org/doc/html/rfc6749#section-2.3.1) (partial implementation)
        .route("/token", post(token_handler))
        // POST `/revoke` [RFC 7009](https://datatracker.ietf.org/doc/html/rfc7009)
        .route("/revoke", post(revoke_handler))
        // POST `/introspect` [RFC 7662](https://datatracker.ietf.org/doc/html/rfc7662)
        .route("/introspect", post(introspect_handler))
        // Custom registration
        .route("/register", post(register_handler))
        .with_state(state)
}

/// [RFC 7517](https://datatracker.ietf.org/doc/html/rfc7517) JWKS endpoint
pub fn jwks_router(state: Arc<AppState>) -> Router {
    Router::new().route("/jwks.json", get(jwks_handler)).with_state(state)
}

async fn jwks_handler(State(state): State<Arc<AppState>>) -> Result<Json<JwksResponse>, StatusCode> {
    let Some(raw_key) = extract_ed25519_public_key(&state.public_key_pem) else {
        error!("cannot parse ed25519 public key");
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };

    let kid = {
        let mut hasher = Sha256::new();
        hasher.update(&raw_key);
        URL_SAFE_NO_PAD.encode(hasher.finalize())
    };

    let x = URL_SAFE_NO_PAD.encode(&raw_key);

    Ok(Json(JwksResponse {
        keys: vec![Jwk {
            kty: "OKP".into(),
            crv: "Ed25519".into(),
            use_: "sig".into(),
            alg: "EdDSA".into(),
            kid,
            x,
        }],
    }))
}

/// Extract the raw 32-byte Ed25519 public key from a PEM-encoded SPKI document
fn extract_ed25519_public_key(pem_bytes: &[u8]) -> Option<Vec<u8>> {
    let der = pem::parse(pem_bytes).ok()?.into_contents();

    match der.len() >= 32 {
        true => Some(der[der.len() - 32..].to_vec()),
        false => None,
    }
}

async fn token_handler(State(state): State<Arc<AppState>>, Form(body): Form<TokenRequest>) -> Result<Json<TokenResponse>, HandlerError> {
    match body {
        // Password grant
        TokenRequest::Password { username, password } => {
            let user = users::Entity::find()
                .filter(users::Column::Username.eq(&username))
                .one(&state.db)
                .await
                .map_err(|e| {
                    error!(?e, %username, "db error looking up user");
                    internal_error()
                })?
                .ok_or_else(|| {
                    warn!(%username, "login attempt for unknown user");
                    invalid_grant()
                })?;

            let valid = hash::verify(&password, &user.password).unwrap_or(false);
            if !valid {
                warn!(user_id = %user.id, "invalid password");
                return Err(invalid_grant());
            }

            let sub = user.id.to_string();

            let access_token = token::generate(&sub, TokenType::Access, &state.private_key_pem, state.access_token_ttl_secs).map_err(|e| {
                error!(?e, user_id = %user.id, "failed to generate access token");
                internal_error()
            })?;

            let refresh_token = token::generate(&sub, TokenType::Refresh, &state.private_key_pem, state.refresh_token_ttl_secs).map_err(|e| {
                error!(?e, user_id = %user.id, "failed to generate refresh token");
                internal_error()
            })?;

            let token_hash = hash::hash(&refresh_token).map_err(|e| {
                error!(?e, user_id = %user.id, "failed to hash refresh token");
                internal_error()
            })?;

            let now = Utc::now();
            refresh_tokens::ActiveModel {
                id: Set(uuid::Uuid::now_v7()),
                user_id: Set(user.id),
                token: Set(token_hash),
                created_at: Set(now),
                expired_at: Set(now + chrono::Duration::seconds(state.refresh_token_ttl_secs as i64)),
            }
            .insert(&state.db)
            .await
            .map_err(|e| {
                error!(?e, user_id = %user.id, "failed to store refresh token");
                internal_error()
            })?;

            info!(user_id = %user.id, "password grant succeeded");

            Ok(Json(TokenResponse::Password {
                access_token,
                token_type: AccessTokenType::Bearer,
                expires_in: state.access_token_ttl_secs,
                refresh_token,
            }))
        }

        // Refresh token grant
        TokenRequest::RefreshToken { refresh_token } => {
            let data = token::verify(&refresh_token, TokenType::Refresh, &state.public_key_pem).map_err(|_| invalid_grant())?;

            let user_id = uuid::Uuid::parse_str(&data.claims.sub).map_err(|_| invalid_grant())?;

            let tokens = refresh_tokens::Entity::find()
                .filter(refresh_tokens::Column::UserId.eq(user_id))
                .all(&state.db)
                .await
                .map_err(|e| {
                    error!(?e, %user_id, "db error looking up refresh tokens");
                    internal_error()
                })?;

            let stored = tokens.iter().find(|t| hash::verify(&refresh_token, &t.token).unwrap_or(false)).ok_or_else(|| {
                warn!(%user_id, "refresh token not found in store");
                invalid_grant()
            })?;

            if stored.expired_at < Utc::now() {
                warn!(user_id = %stored.user_id, "expired refresh token used");
                return Err(invalid_grant());
            }

            let access_token = token::generate(&data.claims.sub, TokenType::Access, &state.private_key_pem, state.access_token_ttl_secs).map_err(|e| {
                error!(?e, sub = %data.claims.sub, "failed to generate access token");
                internal_error()
            })?;

            info!(user_id = %stored.user_id, "refresh grant succeeded");

            Ok(Json(TokenResponse::Refresh {
                access_token,
                token_type: AccessTokenType::Bearer,
                expires_in: state.access_token_ttl_secs,
            }))
        }
    }
}

fn invalid_grant() -> HandlerError {
    (StatusCode::BAD_REQUEST, Json(TokenOperationErrorResponse::InvalidGrant))
}

fn internal_error() -> HandlerError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(TokenOperationErrorResponse::ServerError))
}

/// [RFC 7009#2.2](https://datatracker.ietf.org/doc/html/rfc7009#section-2.2)
async fn revoke_handler(State(state): State<Arc<AppState>>, Form(body): Form<TokenRevocationRequest>) -> StatusCode {
    let Ok(data) = token::verify(&body.token, TokenType::Refresh, &state.public_key_pem) else {
        warn!("revoke called with invalid or expired refresh token");
        return StatusCode::OK;
    };

    let Ok(user_id) = uuid::Uuid::parse_str(&data.claims.sub) else {
        warn!(sub = %data.claims.sub, "revoke called with non-UUID sub");
        return StatusCode::OK;
    };

    let tokens = match refresh_tokens::Entity::find()
        .filter(refresh_tokens::Column::UserId.eq(user_id))
        .all(&state.db)
        .await
    {
        Ok(t) => t,
        Err(e) => {
            error!(?e, %user_id, "db error fetching refresh tokens for revoke");
            return StatusCode::OK;
        }
    };

    let Some(stored) = tokens.iter().find(|t| hash::verify(&body.token, &t.token).unwrap_or(false)) else {
        warn!(%user_id, "revoke called with unknown refresh token");
        return StatusCode::OK;
    };

    // Delete the stored refresh token
    if let Err(e) = refresh_tokens::Entity::delete_by_id(stored.id).exec(&state.db).await {
        error!(?e, token_id = %stored.id, "failed to delete refresh token during revoke");
    } else {
        info!(user_id = %stored.user_id, token_id = %stored.id, "refresh token revoked");
    }

    StatusCode::OK
}

/// [RFC 7662#2.2](https://datatracker.ietf.org/doc/html/rfc7662#section-2.2)
async fn introspect_handler(State(state): State<Arc<AppState>>, Form(body): Form<TokenIntrospectionRequest>) -> Json<TokenIntrospectionResponse> {
    let inactive = || Json(TokenIntrospectionResponse::failed());

    let Ok(data) = token::verify(&body.token, body.token_hint, &state.public_key_pem) else {
        return inactive();
    };

    // For refresh tokens, additionally check the token hasn't been revoked
    if body.token_hint == TokenType::Refresh {
        let Ok(user_id) = uuid::Uuid::parse_str(&data.claims.sub) else {
            return inactive();
        };

        let tokens = match refresh_tokens::Entity::find()
            .filter(refresh_tokens::Column::UserId.eq(user_id))
            .all(&state.db)
            .await
        {
            Ok(t) => t,
            Err(e) => {
                error!(?e, %user_id, "db error during introspection");
                return inactive();
            }
        };

        if !tokens.iter().any(|t| hash::verify(&body.token, &t.token).unwrap_or(false)) {
            warn!(%user_id, "refresh token not found in store");
            return inactive();
        }
    }

    // Look up the user to return the username
    let Ok(user_id) = uuid::Uuid::parse_str(&data.claims.sub) else {
        return inactive();
    };

    let user = match users::Entity::find_by_id(user_id).one(&state.db).await {
        Ok(Some(u)) => u,
        Ok(None) => {
            warn!(%user_id, "user not found");
            return inactive();
        }
        Err(e) => {
            error!(?e, %user_id, "db error looking up user during introspection");
            return inactive();
        }
    };

    Json(TokenIntrospectionResponse::successful(user.username, body.token_hint))
}

type RegisterHandlerError = (StatusCode, Json<RegisterErrorResponse>);

pub async fn register_handler(
    State(state): State<Arc<AppState>>,
    Json(request): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, RegisterHandlerError> {
    // Validate input
    if request.username.trim().is_empty() || request.password.trim().is_empty() || request.email.trim().is_empty() {
        return Err(invalid_register_request());
    }
    if request.password.len() < 8 {
        return Err(invalid_register_request());
    }

    // Check for existing username
    if users::Entity::find_by_username(&request.username)
        .one(&state.db)
        .await
        .map_err(|e| {
            error!(?e, "db error checking username");
            register_server_error()
        })?
        .is_some()
    {
        return Err(username_taken());
    }

    // Check for existing email
    if users::Entity::find_by_email(&request.email)
        .one(&state.db)
        .await
        .map_err(|e| {
            error!(?e, "db error checking email");
            register_server_error()
        })?
        .is_some()
    {
        return Err(email_taken());
    }

    let password_hash = hash::hash(&request.password).map_err(|e| {
        error!(?e, "failed to hash password");
        register_server_error()
    })?;

    let model = users::ActiveModel {
        username: Set(request.username.clone()),
        email: Set(request.email.clone()),
        password: Set(password_hash),
        ..Default::default()
    };

    let result = match model.insert(&state.db).await {
        Ok(user) => user,
        Err(e) => {
            // Handle race condition
            if users::Entity::find_by_username(&request.username).one(&state.db).await.ok().flatten().is_some() {
                return Err(username_taken());
            }
            if users::Entity::find_by_email(&request.email).one(&state.db).await.ok().flatten().is_some() {
                return Err(email_taken());
            }
            error!(?e, "failed to insert user");
            return Err(register_server_error());
        }
    };

    let access_token = token::generate(&result.id.to_string(), TokenType::Access, &state.private_key_pem, state.access_token_ttl_secs).map_err(|e| {
        error!(?e, user_id = %result.id, "failed to generate access token");
        register_server_error()
    })?;

    let refresh_token = token::generate(&result.id.to_string(), TokenType::Refresh, &state.private_key_pem, state.refresh_token_ttl_secs).map_err(|e| {
        error!(?e, user_id = %result.id, "failed to generate refresh token");
        register_server_error()
    })?;

    // Store refresh token hash
    let refresh_token_hash = hash::hash(&refresh_token).map_err(|e| {
        error!(?e, user_id = %result.id, "failed to hash refresh token");
        register_server_error()
    })?;

    let now = Utc::now();
    refresh_tokens::ActiveModel {
        id: Set(uuid::Uuid::now_v7()),
        user_id: Set(result.id),
        token: Set(refresh_token_hash),
        created_at: Set(now),
        expired_at: Set(now + chrono::Duration::seconds(state.refresh_token_ttl_secs as i64)),
    }
    .insert(&state.db)
    .await
    .map_err(|e| {
        error!(?e, user_id = %result.id, "failed to store refresh token");
        register_server_error()
    })?;

    info!(user_id = %result.id, username = %request.username, "user registered");

    Ok(Json(RegisterResponse { access_token, refresh_token }))
}

fn username_taken() -> RegisterHandlerError {
    (StatusCode::CONFLICT, Json(RegisterErrorResponse::UsernameTaken))
}

fn email_taken() -> RegisterHandlerError {
    (StatusCode::CONFLICT, Json(RegisterErrorResponse::EmailTaken))
}

fn invalid_register_request() -> RegisterHandlerError {
    (StatusCode::BAD_REQUEST, Json(RegisterErrorResponse::InvalidRequest))
}

fn register_server_error() -> RegisterHandlerError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(RegisterErrorResponse::ServerError))
}
