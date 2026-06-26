use serde::{Deserialize, Serialize};

/// [RFC 6749#7.1](https://datatracker.ietf.org/doc/html/rfc6749#section-7.1)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessTokenType {
    Bearer,
}

/// [RFC 6749#5.2](https://datatracker.ietf.org/doc/html/rfc6749#section-5.2)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum GetAccessTokenErrorResponse {
    InvalidRequest,
    InvalidGrant,
    UnsupportedGrantType,
    /// WARNING: Non-standard
    ServerError,
}

/// Token endpoint request
///
/// [RFC 6749#4.3.2](https://datatracker.ietf.org/doc/html/rfc6749#section-4.3.2)
/// [RFC 6749#6](https://datatracker.ietf.org/doc/html/rfc6749#section-6)
/// *Partial implementation*
#[derive(Debug, Deserialize)]
#[serde(tag = "grant_type", rename_all = "snake_case")]
pub enum TokenRequest {
    Password { username: String, password: String },
    RefreshToken { refresh_token: String },
}

/// Token endpoint response
///
/// [RFC 6749#5.1](https://datatracker.ietf.org/doc/html/rfc6749#section-5.1)
/// *Partial implementation*
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum TokenResponse {
    Password {
        access_token: String,
        token_type: AccessTokenType,
        /// The lifetime in seconds of the access token
        expires_in: u32,
        refresh_token: String,
    },
    Refresh {
        access_token: String,
        token_type: AccessTokenType,
        /// The lifetime in seconds of the access token
        expires_in: u32,
    },
}
