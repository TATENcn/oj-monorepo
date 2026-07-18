use serde::{Deserialize, Serialize};

use crate::token::TokenType;

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
pub enum TokenOperationErrorResponse {
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
        expires_in: u64,
        refresh_token: String,
    },
    Refresh {
        access_token: String,
        token_type: AccessTokenType,
        /// The lifetime in seconds of the access token
        expires_in: u64,
    },
}

/// [RFC 7009#2.1](https://datatracker.ietf.org/doc/html/rfc7009#section-2.1)
/// *Partial implementation*
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenRevocationRequest {
    pub token: String,
}

/// [RFC 7662#2.1](https://datatracker.ietf.org/doc/html/rfc7662#section-2.1)
/// *Modified implementation*
#[derive(Debug, Serialize, Deserialize)]
pub struct TokenIntrospectionRequest {
    pub token: String,
    pub token_hint: TokenType,
}

/// [RFC 7662#2.2](https://datatracker.ietf.org/doc/html/rfc7662#section-2.2)
/// *Modified implementation*
#[derive(Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum TokenIntrospectionResponse {
    Successful { active: bool, username: String, token_type: TokenType },
    Failed { active: bool },
}

impl TokenIntrospectionResponse {
    pub fn successful(username: String, token_type: TokenType) -> Self {
        Self::Successful {
            active: true,
            username,
            token_type,
        }
    }

    pub fn failed() -> Self {
        Self::Failed { active: false }
    }
}

/// [RFC 7517](https://datatracker.ietf.org/doc/html/rfc7517)
#[derive(Debug, Serialize, Deserialize)]
pub struct JwksResponse {
    pub keys: Vec<Jwk>,
}

/// [RFC 7517#4](https://datatracker.ietf.org/doc/html/rfc7517#section-4)
#[derive(Debug, Serialize, Deserialize)]
pub struct Jwk {
    /// Key type (such as `RSA` and `EC`)
    pub kty: String,
    /// The intended use of the public key
    /// `sig` (signature) or `enc` (encryption)
    #[serde(rename = "use")]
    pub use_: String,
    /// Algorithm
    pub alg: String,
    /// Key ID
    pub kid: String,

    /// Which curve to use
    pub crv: String,
    /// Base64 URL-encoded 32-byte public key
    pub x: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub username: String,
    pub password: String,
    pub email: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub access_token: String,
    pub refresh_token: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum RegisterErrorResponse {
    UsernameTaken,
    EmailTaken,
    InvalidRequest,
    ServerError,
}
