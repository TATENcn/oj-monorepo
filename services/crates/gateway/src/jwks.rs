use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use auth::models::http::JwksResponse;
use base64::Engine;
use http_body_util::BodyExt;
use tokio::task::JoinHandle;
use tokio::time;
use tracing::{info, warn};

use crate::HTTP_CLIENT;

#[derive(Debug)]
struct CachedKey {
    raw_key: Vec<u8>,
}

#[derive(Debug)]
struct JwksCache {
    keys: HashMap<String, CachedKey>,
}

#[derive(Debug, thiserror::Error)]
pub enum JwksError {
    #[error("failed to fetch JWKS: {0}")]
    FetchFailed(#[from] hyper::Error),
    #[error("invalid JWKS URL: {0}")]
    InvalidUrl(#[from] hyper::http::uri::InvalidUri),
    #[error("failed to parse JWKS response: {0}")]
    ParseFailed(#[from] serde_json::Error),
    #[error("JWKS keys not loaded yet")]
    KeysNotLoaded,
    #[error("unknown key id: {0}")]
    UnknownKid(String),
    #[error("JWT verification failed: {0}")]
    TokenVerificationFailed(#[from] jsonwebtoken::errors::Error),
    #[error("failed to decode base64 key: {0}")]
    Base64DecodeFailed(#[from] base64::DecodeError),
    #[error("failed to send HTTP request: {0}")]
    HttpError(#[from] hyper_util::client::legacy::Error),
    #[error("malformed JWT: {0}")]
    MalformedToken(String),
}

pub struct JwksManager {
    cache: Arc<RwLock<Option<JwksCache>>>,
    jwks_url: String,
    refresh_interval: Duration,
    refresh_handle: Option<JoinHandle<()>>,
}

impl JwksManager {
    pub async fn new(jwks_url: String, refresh_interval_secs: Duration) -> Result<Self, JwksError> {
        let manager = Self {
            cache: Arc::new(RwLock::new(None)),
            jwks_url,
            refresh_interval: refresh_interval_secs,
            refresh_handle: None,
        };
        manager.refresh().await?;
        Ok(manager)
    }

    pub fn start_background_refresh(&mut self) {
        let cache = self.cache.clone();
        let jwks_url = self.jwks_url.clone();
        let interval = self.refresh_interval;

        let handle = tokio::spawn(async move {
            let mut timer = time::interval(interval);

            loop {
                timer.tick().await;

                match Self::fetch_jwks(&jwks_url).await {
                    Ok(keys) => {
                        let mut c = cache.write().unwrap();
                        *c = Some(JwksCache { keys });
                        info!("JWKS refreshed");
                    }
                    Err(e) => {
                        warn!(?e, "failed to refresh JWKS");
                    }
                }
            }
        });

        self.refresh_handle = Some(handle);
    }

    /// Verify a JWT token
    pub fn verify(&self, token: &str) -> Result<auth::token::Claims, JwksError> {
        let header = jsonwebtoken::decode_header(token).map_err(|e| JwksError::MalformedToken(format!("failed to decode JWT header: {e}")))?;

        let raw_key = {
            let cache = self.cache.read().unwrap();
            let cache = cache.as_ref().ok_or(JwksError::KeysNotLoaded)?;
            Self::lookup_key(cache, &header)?
        };

        let data = auth::token::verify_with_raw_key(token, auth::token::TokenType::Access, &raw_key)?;

        Ok(data.claims)
    }

    fn lookup_key(cache: &JwksCache, header: &jsonwebtoken::Header) -> Result<Vec<u8>, JwksError> {
        match &header.kid {
            Some(kid) => cache.keys.get(kid).map(|k| k.raw_key.clone()).ok_or_else(|| JwksError::UnknownKid(kid.clone())),
            None => {
                if cache.keys.len() == 1 {
                    Ok(cache.keys.values().next().unwrap().raw_key.clone())
                } else {
                    Err(JwksError::MalformedToken("missing kid in JWT header and JWKS has multiple keys".into()))
                }
            }
        }
    }

    /// Returns true once the JWKS has been loaded
    pub fn is_ready(&self) -> bool {
        self.cache.read().unwrap().is_some()
    }

    async fn refresh(&self) -> Result<(), JwksError> {
        let keys = Self::fetch_jwks(&self.jwks_url).await?;
        let mut cache = self.cache.write().unwrap();
        *cache = Some(JwksCache { keys });
        info!("JWKS loaded successfully");
        Ok(())
    }

    async fn fetch_jwks(url: &str) -> Result<HashMap<String, CachedKey>, JwksError> {
        let uri = url.parse()?;
        let res = HTTP_CLIENT.get(uri).await?;
        let body = res.into_body().collect().await?.to_bytes();
        let jwks: JwksResponse = serde_json::from_slice(&body)?;

        let mut keys = HashMap::new();
        for entry in jwks.keys {
            let raw_key = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(&entry.x)?;
            keys.insert(entry.kid, CachedKey { raw_key });
        }

        Ok(keys)
    }
}

impl Drop for JwksManager {
    fn drop(&mut self) {
        if let Some(handle) = self.refresh_handle.take() {
            handle.abort();
            info!("JWKS background refresh aborted");
        }
    }
}
