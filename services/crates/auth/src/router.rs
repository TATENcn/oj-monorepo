use axum::{Form, Json, Router, extract::State, http::StatusCode, routing::post};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use tracing::{error, info, warn};

use crate::{
    hash,
    models::{
        http::{AccessTokenType, GetAccessTokenErrorResponse, TokenRequest, TokenResponse},
        refresh_tokens, users,
    },
    token::{self, TokenType},
};

#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub private_key_pem: Vec<u8>,
    pub public_key_pem: Vec<u8>,
    pub access_token_ttl: u64,
    pub refresh_token_ttl: u64,
}

type HandlerError = (StatusCode, Json<GetAccessTokenErrorResponse>);

pub fn router(state: AppState) -> Router {
    Router::new()
        // POST `/token` [RFC 6749#2.3.1](https://datatracker.ietf.org/doc/html/rfc6749#section-2.3.1) (partial implementation)
        .route("/token", post(token_handler))
        .with_state(state)
}

async fn token_handler(State(state): State<AppState>, Form(body): Form<TokenRequest>) -> Result<Json<TokenResponse>, HandlerError> {
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

            let access_token = token::generate(&sub, TokenType::Access, &state.private_key_pem, state.access_token_ttl).map_err(|e| {
                error!(?e, user_id = %user.id, "failed to generate access token");
                internal_error()
            })?;

            let refresh_token = token::generate(&sub, TokenType::Refresh, &state.private_key_pem, state.refresh_token_ttl).map_err(|e| {
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
                expired_at: Set(now + chrono::Duration::seconds(state.refresh_token_ttl as i64)),
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
                expires_in: state.access_token_ttl,
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

            let access_token = token::generate(&data.claims.sub, TokenType::Access, &state.private_key_pem, state.access_token_ttl).map_err(|e| {
                error!(?e, sub = %data.claims.sub, "failed to generate access token");
                internal_error()
            })?;

            info!(user_id = %stored.user_id, "refresh grant succeeded");

            Ok(Json(TokenResponse::Refresh {
                access_token,
                token_type: AccessTokenType::Bearer,
                expires_in: state.access_token_ttl,
            }))
        }
    }
}

fn invalid_grant() -> HandlerError {
    (StatusCode::BAD_REQUEST, Json(GetAccessTokenErrorResponse::InvalidGrant))
}

fn internal_error() -> HandlerError {
    (StatusCode::INTERNAL_SERVER_ERROR, Json(GetAccessTokenErrorResponse::ServerError))
}
