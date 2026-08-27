//! Process configuration, read from the environment (and a local `.env` when present).

use std::net::SocketAddr;
use std::path::Path;
use std::str::FromStr;

use rust_decimal::Decimal;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("missing required environment variable {0}")]
    Missing(&'static str),
    #[error("invalid value for {0}: {1}")]
    Invalid(&'static str, String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFormat {
    Json,
    Pretty,
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub endpoint: Option<String>,
    pub region: String,
    pub bucket_documents: String,
    pub bucket_pdfs: String,
    pub access_key_id: String,
    pub secret_access_key: String,
    pub force_path_style: bool,
    pub presign_ttl_seconds: u64,
}

#[derive(Debug, Clone)]
pub struct SeedConfig {
    pub password: String,
    pub skip_password_change: bool,
    pub random_seed: u64,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub database_max_connections: u32,
    pub database_migrate_on_start: bool,
    pub redis_url: Option<String>,
    pub api_bind: SocketAddr,
    pub api_public_url: String,
    pub cors_origins: Vec<String>,
    pub jwt_secret: String,
    pub jwt_issuer: String,
    pub access_token_ttl_seconds: u64,
    pub refresh_token_ttl_seconds: u64,
    pub login_max_failures: i32,
    pub login_lockout_seconds: i64,
    pub rate_limit_per_minute: u32,
    pub invoice_approval_threshold: Decimal,
    pub billing_url: String,
    pub analytics_url: String,
    pub internal_service_token: String,
    pub log_format: LogFormat,
    pub s3: S3Config,
    pub seed: SeedConfig,
}

impl Config {
    /// Builds the configuration from the process environment. A `.env` file in the
    /// working directory or its parent is loaded first as a development convenience;
    /// variables already present in the environment always win.
    pub fn from_env() -> Result<Config, ConfigError> {
        load_dotenv();
        let s3 = S3Config {
            endpoint: optional("S3_ENDPOINT").filter(|s| !s.is_empty()),
            region: optional("S3_REGION").unwrap_or_else(|| "us-east-1".to_string()),
            bucket_documents: optional("S3_BUCKET_DOCUMENTS")
                .unwrap_or_else(|| "bowline-documents".to_string()),
            bucket_pdfs: optional("S3_BUCKET_PDFS").unwrap_or_else(|| "bowline-pdfs".to_string()),
            access_key_id: optional("S3_ACCESS_KEY_ID").unwrap_or_default(),
            secret_access_key: optional("S3_SECRET_ACCESS_KEY").unwrap_or_default(),
            force_path_style: flag("S3_FORCE_PATH_STYLE", true)?,
            presign_ttl_seconds: parsed("PRESIGN_TTL_SECONDS", 900)?,
        };
        let seed = SeedConfig {
            password: optional("SEED_PASSWORD").unwrap_or_else(|| "Bowline!2026".to_string()),
            skip_password_change: flag("SEED_SKIP_PASSWORD_CHANGE", true)?,
            random_seed: parsed("SEED_RANDOM_SEED", 42)?,
        };
        let jwt_secret = required("JWT_SECRET")?;
        if jwt_secret.len() < 32 {
            return Err(ConfigError::Invalid(
                "JWT_SECRET",
                "must be at least 32 characters".to_string(),
            ));
        }
        Ok(Config {
            database_url: required("DATABASE_URL")?,
            database_max_connections: parsed("DATABASE_MAX_CONNECTIONS", 20)?,
            database_migrate_on_start: flag("DATABASE_MIGRATE_ON_START", true)?,
            redis_url: optional("REDIS_URL").filter(|s| !s.is_empty()),
            api_bind: parsed("API_BIND", SocketAddr::from(([0, 0, 0, 0], 8080)))?,
            api_public_url: optional("API_PUBLIC_URL")
                .unwrap_or_else(|| "http://localhost:8080".to_string()),
            cors_origins: optional("API_CORS_ORIGINS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            jwt_secret,
            jwt_issuer: optional("JWT_ISSUER").unwrap_or_else(|| "bowline".to_string()),
            access_token_ttl_seconds: parsed("ACCESS_TOKEN_TTL_SECONDS", 900)?,
            refresh_token_ttl_seconds: parsed("REFRESH_TOKEN_TTL_SECONDS", 2_592_000)?,
            login_max_failures: parsed("LOGIN_MAX_FAILURES", 5)?,
            login_lockout_seconds: parsed("LOGIN_LOCKOUT_SECONDS", 900)?,
            rate_limit_per_minute: parsed("RATE_LIMIT_PER_MINUTE", 300)?,
            invoice_approval_threshold: parsed(
                "INVOICE_APPROVAL_THRESHOLD",
                Decimal::from(50_000),
            )?,
            billing_url: optional("BILLING_URL").unwrap_or_else(|| "http://localhost:8081".into()),
            analytics_url: optional("ANALYTICS_URL")
                .unwrap_or_else(|| "http://localhost:8082".into()),
            internal_service_token: optional("INTERNAL_SERVICE_TOKEN").unwrap_or_default(),
            log_format: match optional("LOG_FORMAT").as_deref() {
                Some("json") => LogFormat::Json,
                _ => LogFormat::Pretty,
            },
            s3,
            seed,
        })
    }
}

fn load_dotenv() {
    for candidate in [".env", "../.env"] {
        if Path::new(candidate).is_file() {
            let _ = dotenvy::from_filename(candidate);
            return;
        }
    }
}

/// Reads a variable, dropping an inline ` # comment` and surrounding whitespace so
/// the annotated `.env.example` values parse cleanly.
fn optional(name: &'static str) -> Option<String> {
    let raw = std::env::var(name).ok()?;
    let cleaned = match raw.find(" #") {
        Some(idx) => &raw[..idx],
        None => raw.as_str(),
    };
    Some(cleaned.trim().trim_matches('"').to_string())
}

fn required(name: &'static str) -> Result<String, ConfigError> {
    optional(name)
        .filter(|s| !s.is_empty())
        .ok_or(ConfigError::Missing(name))
}

fn parsed<T: FromStr>(name: &'static str, default: T) -> Result<T, ConfigError>
where
    T::Err: std::fmt::Display,
{
    match optional(name) {
        Some(s) if !s.is_empty() => s
            .parse::<T>()
            .map_err(|e| ConfigError::Invalid(name, e.to_string())),
        _ => Ok(default),
    }
}

fn flag(name: &'static str, default: bool) -> Result<bool, ConfigError> {
    match optional(name).as_deref() {
        None | Some("") => Ok(default),
        Some("1") | Some("true") | Some("yes") | Some("on") => Ok(true),
        Some("0") | Some("false") | Some("no") | Some("off") => Ok(false),
        Some(other) => Err(ConfigError::Invalid(
            name,
            format!("not a boolean: {other}"),
        )),
    }
}
