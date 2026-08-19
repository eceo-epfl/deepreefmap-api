use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Deployment {
    Local,
    Dev,
    Stage,
    Prod,
}

impl std::str::FromStr for Deployment {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "dev" | "development" => Ok(Self::Dev),
            "stage" | "staging" => Ok(Self::Stage),
            "prod" | "production" => Ok(Self::Prod),
            "local" | "" => Ok(Self::Local),
            _ => Err(()),
        }
    }
}

impl Deployment {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Dev => "dev",
            Self::Stage => "stage",
            Self::Prod => "prod",
        }
    }
}

/// The blob store behind `/api/archive`. Present only when every `S3_*` variable is
/// set: the archive is one feature, not five independent knobs.
#[derive(Debug, Clone)]
pub struct ArchiveConfig {
    /// Endpoint with a scheme. `S3_URL` may omit it, in which case localhost gets
    /// `http` and everything else `https`.
    pub endpoint_url: String,
    /// `S3_PUBLIC_URL`, the endpoint presigned URLs are signed against, when set.
    ///
    /// For a store clients reach at a different name than the API uses internally:
    /// a host-preserving proxy in front of an intranet-only Scality, or compose
    /// `MinIO` advertised as localhost. Server-side operations (create, complete,
    /// list, abort, verify) stay on `endpoint_url`.
    pub public_endpoint_url: Option<String>,
    pub bucket: String,
    pub access_key: String,
    pub secret_key: String,
    /// Leading path segment of every key, so one bucket can hold several deployments.
    pub prefix: String,
}

impl ArchiveConfig {
    /// Read the `S3_*` group, or `None` when any member is missing.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let vars = [
            "S3_URL",
            "S3_BUCKET_ID",
            "S3_ACCESS_KEY",
            "S3_SECRET_KEY",
            "S3_PREFIX",
        ];
        let values: Vec<Option<String>> = vars.iter().map(|name| optional(name)).collect();
        if values.iter().all(Option::is_none) {
            return None;
        }
        if values.iter().any(Option::is_none) {
            let missing: Vec<&str> = vars
                .iter()
                .zip(&values)
                .filter(|(_, value)| value.is_none())
                .map(|(name, _)| *name)
                .collect();
            tracing::warn!(
                ?missing,
                "Partial S3 configuration: the archive stays disabled"
            );
            return None;
        }
        let mut values = values.into_iter().flatten();
        Some(Self {
            endpoint_url: with_scheme(&values.next()?),
            public_endpoint_url: optional("S3_PUBLIC_URL").map(|url| with_scheme(&url)),
            bucket: values.next()?,
            access_key: values.next()?,
            secret_key: values.next()?,
            prefix: values.next()?.trim_matches('/').to_string(),
        })
    }
}

/// Default a bare `host:port` to a scheme: `http` for localhost, `https` elsewhere.
fn with_scheme(url: &str) -> String {
    let url = url.trim_end_matches('/');
    if url.contains("://") {
        return url.to_string();
    }
    let host = url.split(':').next().unwrap_or(url);
    if host == "localhost" || host == "127.0.0.1" {
        format!("http://{url}")
    } else {
        format!("https://{url}")
    }
}

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,

    pub api_host: String,
    pub api_port: u16,

    pub deployment: Deployment,

    /// Keycloak protects the web interface. Optional so a local checkout runs
    /// without a realm, but required in stage and prod (see `main`).
    pub keycloak_url: Option<String>,
    /// What `/api/config/keycloak` advertises, when it differs from the URL this
    /// server validates against. Both exist because in a container network the
    /// address the API reaches Keycloak on is not one a browser can resolve, and
    /// handing a browser `http://keycloak:8080/` sends it nowhere.
    pub keycloak_browser_url: Option<String>,
    pub keycloak_realm: Option<String>,
    pub keycloak_client_id: Option<String>,

    /// Absolute base URL the desktop application should call, embedded in the
    /// connect codes minted by this server. Without it a code carries no address
    /// and cannot be pasted anywhere useful, so minting fails rather than issuing
    /// a code that looks valid.
    pub public_base_url: Option<String>,

    pub cors_allowed_origins: Vec<String>,

    pub db_max_connections: u32,
    pub db_min_connections: u32,

    pub request_timeout_seconds: u64,

    /// How long a freshly minted connect code stays usable. Short: it is a bearer
    /// secret that travels through chat and email on its way to a laptop.
    pub connect_code_ttl_seconds: i64,

    /// Device-token validation cache TTL. Short so a revocation takes effect
    /// promptly; revoking also busts the cache explicitly.
    pub token_cache_ttl_seconds: u64,

    pub disable_rate_limiting: bool,
    /// Per-IP limit on the unauthenticated enrolment endpoint, where each attempt
    /// costs an argon2 verification.
    pub enrol_rate_limit_burst: u32,
    pub enrol_rate_limit_period_secs: u64,

    /// Ceiling on a sync push body. Metadata only, so a generous cap still leaves
    /// no room for anyone to post a video through it.
    pub sync_body_limit_bytes: usize,

    /// The blob store, when configured. `None` disables every `/archive` route.
    pub archive: Option<ArchiveConfig>,
    /// Sanity ceiling on one archived object, not a policy cap.
    pub archive_max_object_bytes: i64,
    /// How often the reaper sweeps stale uploads and unverified objects.
    pub archive_sweep_seconds: u64,
    /// A pending upload with no part activity for this long is abandoned.
    pub archive_upload_timeout_seconds: i64,
}

/// The defaults `from_env` falls back to, and what the contract exporter runs on: it
/// builds the `OpenAPI` document without a database or a realm.
impl Default for Config {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            api_host: "0.0.0.0".to_string(),
            api_port: 3000,
            deployment: Deployment::Local,
            keycloak_url: None,
            keycloak_browser_url: None,
            keycloak_realm: None,
            keycloak_client_id: None,
            public_base_url: None,
            cors_allowed_origins: vec!["http://localhost:5173".to_string()],
            db_max_connections: 20,
            db_min_connections: 5,
            request_timeout_seconds: 60,
            connect_code_ttl_seconds: 900,
            token_cache_ttl_seconds: 5,
            disable_rate_limiting: false,
            enrol_rate_limit_burst: 5,
            enrol_rate_limit_period_secs: 10,
            sync_body_limit_bytes: 16 * 1024 * 1024,
            archive: None,
            archive_max_object_bytes: 64 * 1024 * 1024 * 1024,
            archive_sweep_seconds: 900,
            archive_upload_timeout_seconds: 86_400,
        }
    }
}

impl Config {
    /// Load configuration from the environment.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError::Missing`] when no database connection can be assembled.
    pub fn from_env() -> Result<Self, ConfigError> {
        dotenvy::dotenv().ok();
        let fallback = Self::default();

        Ok(Self {
            database_url: env::var("DATABASE_URL")
                .or_else(|_| {
                    let user = env::var("DB_USER")?;
                    let password = env::var("DB_PASSWORD")?;
                    let host = env::var("DB_HOST")?;
                    let port = env::var("DB_PORT").unwrap_or_else(|_| "5432".to_string());
                    let name = env::var("DB_NAME")?;
                    Ok::<String, env::VarError>(format!(
                        "postgresql://{user}:{password}@{host}:{port}/{name}"
                    ))
                })
                .map_err(|_| {
                    ConfigError::Missing("DATABASE_URL or DB_USER/DB_PASSWORD/DB_HOST/DB_NAME")
                })?,

            api_host: env::var("API_HOST").unwrap_or(fallback.api_host),
            api_port: parse_or("API_PORT", fallback.api_port),

            deployment: env::var("DEPLOYMENT")
                .unwrap_or_default()
                .parse()
                .unwrap_or(fallback.deployment),

            keycloak_url: optional("KEYCLOAK_URL"),
            keycloak_browser_url: optional("KEYCLOAK_BROWSER_URL"),
            keycloak_realm: optional("KEYCLOAK_REALM"),
            keycloak_client_id: optional("KEYCLOAK_CLIENT_ID"),

            public_base_url: optional("PUBLIC_BASE_URL")
                .map(|s| s.trim_end_matches('/').to_string()),

            cors_allowed_origins: match env::var("CORS_ALLOWED_ORIGINS") {
                Ok(raw) => raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
                Err(_) => fallback.cors_allowed_origins,
            },

            db_max_connections: parse_or("DB_MAX_CONNECTIONS", fallback.db_max_connections),
            db_min_connections: parse_or("DB_MIN_CONNECTIONS", fallback.db_min_connections),

            request_timeout_seconds: parse_or(
                "REQUEST_TIMEOUT_SECONDS",
                fallback.request_timeout_seconds,
            ),

            connect_code_ttl_seconds: parse_or(
                "CONNECT_CODE_TTL_SECONDS",
                fallback.connect_code_ttl_seconds,
            ),

            token_cache_ttl_seconds: parse_or(
                "TOKEN_CACHE_TTL_SECONDS",
                fallback.token_cache_ttl_seconds,
            ),

            disable_rate_limiting: parse_or(
                "DISABLE_RATE_LIMITING",
                fallback.disable_rate_limiting,
            ),
            enrol_rate_limit_burst: parse_or(
                "ENROL_RATE_LIMIT_BURST",
                fallback.enrol_rate_limit_burst,
            ),
            enrol_rate_limit_period_secs: parse_or(
                "ENROL_RATE_LIMIT_PERIOD_SECS",
                fallback.enrol_rate_limit_period_secs,
            ),

            sync_body_limit_bytes: parse_or(
                "SYNC_BODY_LIMIT_BYTES",
                fallback.sync_body_limit_bytes,
            ),

            archive: ArchiveConfig::from_env(),
            archive_max_object_bytes: parse_or(
                "ARCHIVE_MAX_OBJECT_BYTES",
                fallback.archive_max_object_bytes,
            ),
            archive_sweep_seconds: parse_or(
                "ARCHIVE_SWEEP_SECONDS",
                fallback.archive_sweep_seconds,
            ),
            archive_upload_timeout_seconds: parse_or(
                "ARCHIVE_UPLOAD_TIMEOUT_SECONDS",
                fallback.archive_upload_timeout_seconds,
            ),
        })
    }

    #[must_use]
    pub fn bind_address(&self) -> String {
        format!("{}:{}", self.api_host, self.api_port)
    }

    /// Whether Keycloak must be configured for this deployment. Local and dev may
    /// run without a realm; anything user-facing may not.
    #[must_use]
    pub fn requires_keycloak(&self) -> bool {
        matches!(self.deployment, Deployment::Stage | Deployment::Prod)
    }

    /// The `iss` values an accepted JWT may carry. Empty when no realm is configured.
    ///
    /// Both addresses count. Keycloak stamps `iss` from the address the client reached it
    /// on, so a browser token names the browser-facing URL while this server validates
    /// against the one it can resolve itself.
    #[must_use]
    pub fn expected_issuers(&self) -> Vec<String> {
        let Some(realm) = self.keycloak_realm.as_ref() else {
            return Vec::new();
        };
        [
            self.keycloak_url.as_ref(),
            self.keycloak_browser_url.as_ref(),
        ]
        .into_iter()
        .flatten()
        .map(|url| format!("{}/realms/{realm}", url.trim_end_matches('/')))
        .collect()
    }
}

fn optional(key: &str) -> Option<String> {
    env::var(key)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn parse_or<T: std::str::FromStr>(key: &str, default: T) -> T {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    Missing(&'static str),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_keycloak(url: Option<&str>, realm: Option<&str>) -> Config {
        Config {
            keycloak_url: url.map(String::from),
            keycloak_realm: realm.map(String::from),
            ..Config::default()
        }
    }

    #[test]
    fn test_expected_issuer_matches_keycloak_format() {
        let config = with_keycloak(Some("https://sso.example/"), Some("deepreefmap"));
        assert_eq!(
            config.expected_issuers(),
            ["https://sso.example/realms/deepreefmap"]
        );
    }

    /// A browser reaches Keycloak on a different address than this server does, and
    /// Keycloak stamps `iss` from whichever was used.
    #[test]
    fn test_both_keycloak_addresses_are_accepted_issuers() {
        let config = Config {
            keycloak_url: Some("http://deepreefmap-keycloak:8080/".to_string()),
            keycloak_browser_url: Some("http://localhost:8280/".to_string()),
            keycloak_realm: Some("deepreefmap".to_string()),
            ..Config::default()
        };
        assert_eq!(
            config.expected_issuers(),
            [
                "http://deepreefmap-keycloak:8080/realms/deepreefmap",
                "http://localhost:8280/realms/deepreefmap",
            ]
        );
    }

    /// Local `MinIO` speaks plain HTTP; anything with a real hostname must not.
    #[test]
    fn test_bare_s3_url_defaults_scheme_by_host() {
        assert_eq!(with_scheme("localhost:9000"), "http://localhost:9000");
        assert_eq!(with_scheme("127.0.0.1:9000"), "http://127.0.0.1:9000");
        assert_eq!(with_scheme("s3.example.org"), "https://s3.example.org");
        assert_eq!(with_scheme("http://minio:9000/"), "http://minio:9000");
    }

    #[test]
    fn test_no_expected_issuer_without_a_realm() {
        assert!(
            with_keycloak(Some("https://sso.example"), None)
                .expected_issuers()
                .is_empty()
        );
    }
}
