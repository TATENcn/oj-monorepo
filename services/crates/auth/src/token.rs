use std::{collections::HashSet, sync::LazyLock, time};

use ed25519_dalek::pkcs8::EncodePublicKey;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, TokenData, Validation, decode, encode, errors::ErrorKind};
use serde::{Deserialize, Serialize};

const ISSUER: &str = "auth";
const AUDIENCE: &str = "onlinejudge";

static JWKS_VALIDATION: LazyLock<Validation> = LazyLock::new(|| {
    let mut validation = Validation::new(Algorithm::EdDSA);
    validation.aud = Some(HashSet::from([AUDIENCE.to_string()]));
    validation.iss = Some(HashSet::from([ISSUER.to_string()]));
    validation
});

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenType {
    Access,
    Refresh,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    /// Audience
    pub aud: String,
    /// Expiration time (as UTC timestamp in seconds)
    pub exp: u64,
    /// Issued at (as UTC timestamp in seconds)
    pub iat: u64,
    /// Issuer
    pub iss: String,
    /// Not Before (as UTC timestamp in seconds)
    pub nbf: u64,
    /// Subject (whom token refers to)
    pub sub: String,
    /// Token type: [`TokenType::Access`] or [`TokenType::Refresh`]
    pub typ: TokenType,
}

/// Generate a signed JWT
///
/// ### Parameters
/// sub - user id
/// typ - token type (access or refresh)
/// private_key_pem - private key pem
/// expiration_secs - token lifetime
pub fn generate(sub: &str, typ: TokenType, private_key_pem: &[u8], expiration_secs: u64) -> Result<String, jsonwebtoken::errors::Error> {
    let now = time::SystemTime::now()
        .duration_since(time::UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_secs();

    let claims = Claims {
        aud: AUDIENCE.into(),
        exp: now.saturating_add(expiration_secs),
        iat: now,
        iss: ISSUER.into(),
        nbf: now,
        sub: sub.to_string(),
        typ,
    };

    let header = Header::new(Algorithm::EdDSA);

    encode(&header, &claims, &EncodingKey::from_ed_pem(private_key_pem)?)
}

/// Verify and decode a JWT
pub fn verify(token: &str, typ: TokenType, public_key_pem: &[u8]) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
    let data = decode::<Claims>(token, &DecodingKey::from_ed_pem(public_key_pem)?, &JWKS_VALIDATION)?;

    if data.claims.typ != typ {
        return Err(ErrorKind::InvalidToken.into());
    }

    Ok(data)
}

pub fn verify_with_raw_key(token: &str, typ: TokenType, raw_key: &[u8]) -> Result<TokenData<Claims>, jsonwebtoken::errors::Error> {
    let der = raw_to_der(raw_key)?;
    let data = decode::<Claims>(token, &DecodingKey::from_ed_der(&der), &JWKS_VALIDATION)?;

    if data.claims.typ != typ {
        return Err(ErrorKind::InvalidToken.into());
    }

    Ok(data)
}

fn raw_to_der(raw_key: &[u8]) -> Result<Vec<u8>, jsonwebtoken::errors::Error> {
    let arr: &[u8; 32] = raw_key.try_into().map_err(|_| jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)?;
    let verifying_key = ed25519_dalek::VerifyingKey::from_bytes(arr).map_err(|_| jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)?;
    let der = verifying_key
        .to_public_key_der()
        .map_err(|_| jsonwebtoken::errors::ErrorKind::InvalidKeyFormat)?;

    Ok(der.as_bytes().to_vec())
}
