use std::sync::Arc;

use axum::{
    extract::{Extension, FromRequestParts},
    http::{StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use tracing::warn;
use uuid::Uuid;

use crate::{router::AppState, token};

/// Authenticated user identity extracted from a valid JWT
#[derive(Debug, Clone)]
pub struct Identity {
    pub user_id: Uuid,
}

/// Extracts `x-user-id` header injected by an upstream gateway
///
/// [`UserId<Identity>`] requires the header to be present and parseable missing or malformed values produce a 500
///
/// [`UserId<Option<Identity>>`] yields [`Option::None`] when the header is absent
#[derive(Debug)]
pub struct UserId<T>(pub T);

impl<S> FromRequestParts<S> for UserId<Identity>
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let user_id = extract_x_user_id(parts.headers.get("x-user-id"))?;
        Ok(UserId(Identity { user_id }))
    }
}

impl<S> FromRequestParts<S> for UserId<Option<Identity>>
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        match parts.headers.get("x-user-id") {
            Some(val) => {
                let user_id = extract_x_user_id(Some(val))?;
                Ok(UserId(Some(Identity { user_id })))
            }
            None => Ok(UserId(None)),
        }
    }
}

fn extract_x_user_id(header: Option<&axum::http::HeaderValue>) -> Result<Uuid, Response> {
    let val = header.ok_or_else(|| {
        warn!("x-user-id header missing from gateway");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;
    let s = val.to_str().map_err(|_| {
        warn!("x-user-id header is not valid UTF-8");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })?;
    Uuid::parse_str(s).map_err(|_| {
        warn!(?s, "x-user-id header is not a valid UUID");
        StatusCode::INTERNAL_SERVER_ERROR.into_response()
    })
}

/// NOTES:
/// - [`Auth<Identity>`] requires a valid access token
/// - [`Auth<Option<Identity>>`] yields [`Option::None`] when no header is present (but still rejects malformed or expired tokens)
#[derive(Debug)]
pub struct Auth<T>(pub T);

impl<S> FromRequestParts<S> for Auth<Identity>
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_state = get_auth_state(parts, state).await?;
        let identity = extract_identity(parts, &auth_state).await?;
        Ok(Auth(identity))
    }
}

impl<S> FromRequestParts<S> for Auth<Option<Identity>>
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let auth_state = get_auth_state(parts, state).await?;

        let Some(auth_header) = parts.headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()) else {
            return Ok(Auth(None));
        };

        let identity = verify_bearer(auth_header, &auth_state)?;

        Ok(Auth(Some(identity)))
    }
}

/// [RFC 6750#3.1](https://datatracker.ietf.org/doc/html/rfc6750#section-3.1)
#[derive(Debug, Clone, Copy)]
pub enum AuthError {
    StateNotConfigured,
    MissingHeader,
    InvalidHeader,
    InvalidToken,
    BadSubject,
}

impl From<AuthError> for StatusCode {
    fn from(val: AuthError) -> Self {
        match val {
            AuthError::StateNotConfigured | AuthError::BadSubject => StatusCode::INTERNAL_SERVER_ERROR,
            AuthError::MissingHeader | AuthError::InvalidHeader | AuthError::InvalidToken => StatusCode::UNAUTHORIZED,
        }
    }
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        let mut res = Into::<StatusCode>::into(self).into_response();

        if let Some(err) = self.www_auth_error() {
            let www_auth = WwwAuthenticate::bearer("api", err);
            res.headers_mut().insert(header::WWW_AUTHENTICATE, www_auth.to_header_value());
        }

        res
    }
}

impl AuthError {
    /// [RFC 6750#3.1](https://datatracker.ietf.org/doc/html/rfc6750#section-3.1)
    fn www_auth_error(self) -> Option<WwwAuthError> {
        match self {
            Self::StateNotConfigured | Self::BadSubject => None,
            Self::MissingHeader | Self::InvalidHeader | Self::InvalidToken => Some(WwwAuthError::InvalidToken),
        }
    }
}

/// [RFC 6750#3.1](https://datatracker.ietf.org/doc/html/rfc6750#section-3.1)
#[derive(Debug, Clone, Copy)]
pub enum WwwAuthError {
    InvalidToken,
    InvalidRequest,
    InsufficientScope,
}

impl WwwAuthError {
    fn as_str(self) -> &'static str {
        match self {
            Self::InvalidToken => "invalid_token",
            Self::InvalidRequest => "invalid_request",
            Self::InsufficientScope => "insufficient_scope",
        }
    }
}

/// [RFC 7235](https://datatracker.ietf.org/doc/html/rfc7235#section-4.1),
/// [RFC 6750#3.1](https://datatracker.ietf.org/doc/html/rfc6750#section-3.1)
/// `WWW-Authenticate` header builder
#[derive(Debug, Clone)]
pub struct WwwAuthenticate {
    scheme: &'static str,
    params: Vec<(&'static str, &'static str)>,
}

impl WwwAuthenticate {
    pub fn bearer(realm: &'static str, error: WwwAuthError) -> Self {
        Self {
            scheme: "Bearer",
            params: vec![("realm", realm), ("error", error.as_str())],
        }
    }

    pub fn to_header_value(&self) -> axum::http::HeaderValue {
        let value = self.params.iter().map(|(k, v)| format!("{k}=\"{v}\"")).collect::<Vec<_>>().join(", ");
        format!("{} {}", self.scheme, value).parse().unwrap()
    }
}

async fn get_auth_state<S: Send + Sync>(parts: &mut Parts, state: &S) -> Result<Arc<AppState>, Response> {
    Extension::<Arc<AppState>>::from_request_parts(parts, state)
        .await
        .map(|e| e.0)
        .map_err(|_| AuthError::StateNotConfigured.into_response())
}

async fn extract_identity(parts: &mut Parts, auth_state: &Arc<AppState>) -> Result<Identity, Response> {
    let auth_header = parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| AuthError::MissingHeader.into_response())?;

    verify_bearer(auth_header, auth_state)
}

fn verify_bearer(auth_header: &str, auth_state: &Arc<AppState>) -> Result<Identity, Response> {
    let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| AuthError::InvalidHeader.into_response())?;
    // [RFC 6749#1.5](https://datatracker.ietf.org/doc/html/rfc6749#section-1.5)
    let data = token::verify(token, token::TokenType::Access, &auth_state.public_key_pem).map_err(|_| AuthError::InvalidToken.into_response())?;

    let user_id = Uuid::parse_str(&data.claims.sub).map_err(|_| AuthError::BadSubject.into_response())?;

    Ok(Identity { user_id })
}
