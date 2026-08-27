//! HS256 access tokens carrying only the user id and the token version.

use chrono::Utc;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::config::Config;
use crate::error::{ApiError, ApiResult};

#[derive(Debug, Serialize, Deserialize)]
pub struct Claims {
    pub sub: Uuid,
    pub tv: i32,
    pub iat: i64,
    pub exp: i64,
    pub iss: String,
}

pub fn encode_access(config: &Config, user_id: Uuid, token_version: i32) -> ApiResult<String> {
    let now = Utc::now().timestamp();
    let claims = Claims {
        sub: user_id,
        tv: token_version,
        iat: now,
        exp: now + config.access_token_ttl_seconds as i64,
        iss: config.jwt_issuer.clone(),
    };
    jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(config.jwt_secret.as_bytes()),
    )
    .map_err(|e| ApiError::internal_msg(format!("token encoding failed: {e}")))
}

pub fn decode_access(config: &Config, token: &str) -> ApiResult<Claims> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.set_issuer(&[config.jwt_issuer.as_str()]);
    validation.leeway = 5;
    jsonwebtoken::decode::<Claims>(
        token,
        &DecodingKey::from_secret(config.jwt_secret.as_bytes()),
        &validation,
    )
    .map(|data| data.claims)
    .map_err(|e| ApiError::Unauthorized(format!("invalid access token: {e}")))
}
