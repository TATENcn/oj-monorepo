use serde::{Deserialize, Serialize};

/// [RFC 6749#4.3.2](https://datatracker.ietf.org/doc/html/rfc6749#section-4.3.2)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantType {
    Password,
    RefreshToken,
}

/// [RFC 6749#7.1](https://datatracker.ietf.org/doc/html/rfc6749#section-7.1)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessTokenType {
    Bearer,
}

/// [RFC 6749#4.3.2](https://datatracker.ietf.org/doc/html/rfc6749#section-4.3.2)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetAccessTokenByPasswordRequest {
    /// Must be [`GrantType::Password`]
    pub grant_type: GrantType,
    pub password: String,
    pub username: String,
}

/// [RFC 6749#6](https://datatracker.ietf.org/doc/html/rfc6749#section-6)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetAccessTokenByRefreshTokenRequest {
    /// Must be [`GrantType::RefreshToken`]
    pub grant_type: GrantType,
    pub refresh_token: String,
}

/// [RFC 6749#5.1](https://datatracker.ietf.org/doc/html/rfc6749#section-5.1)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetAccessTokenByPasswordSuccessfulResponse {
    pub access_token: String,
    pub token_type: AccessTokenType,
    /// The lifetime in seconds of the access token
    pub expires_in: u32,
    pub refresh_token: String,
}

/// [RFC 6749#5.1](https://datatracker.ietf.org/doc/html/rfc6749#section-5.1)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct GetAccessTokenByRefreshTokenSuccessfulResponse {
    pub access_token: String,
    pub token_type: AccessTokenType,
    /// The lifetime in seconds of the access token
    pub expires_in: u32,
}

/// [RFC 6749#5.2](https://datatracker.ietf.org/doc/html/rfc6749#section-5.2)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum GetAccessTokenErrorResponse {
    InvalidRequest,
    InvalidGrant,
    UnsupportedGrantType,
}
