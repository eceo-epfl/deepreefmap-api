//! Shared test harness.
//!
//! Drives the real router, so authentication, body limits and route wiring are all
//! exercised. No Keycloak is configured, so tests authenticate as devices.

pub mod client;
pub mod db;

pub use client::*;
pub use db::*;

use deepreefmap_api::common::AppState;
use deepreefmap_api::common::auth::{AuthContext, Role};
use deepreefmap_api::config::{Config, Deployment};
use sea_orm::DatabaseConnection;

/// A stamp newer than a row just written, and inside the clock skew a push allows.
#[allow(dead_code)]
#[must_use]
pub fn soon() -> String {
    (chrono::Utc::now() + chrono::TimeDelta::minutes(1))
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[must_use]
pub fn test_config() -> Config {
    Config {
        database_url: std::env::var("DATABASE_URL").unwrap_or_default(),
        api_host: "127.0.0.1".to_string(),
        api_port: 0,
        deployment: Deployment::Local,
        keycloak_url: None,
        keycloak_browser_url: None,
        keycloak_realm: None,
        keycloak_client_id: None,
        public_base_url: Some("http://test.local/api".to_string()),
        cors_allowed_origins: vec![],
        db_max_connections: 5,
        db_min_connections: 1,
        request_timeout_seconds: 30,
        archive_request_timeout_seconds: 600,
        connect_code_ttl_seconds: 900,
        // Zero falls back to the default, and a short TTL keeps revocation honest.
        token_cache_ttl_seconds: 1,
        disable_rate_limiting: true,
        enrol_rate_limit_burst: 100,
        enrol_rate_limit_period_secs: 1,
        sync_body_limit_bytes: 16 * 1024 * 1024,
        archive: None,
        archive_max_object_bytes: 64 * 1024 * 1024 * 1024,
        archive_sweep_seconds: 900,
        archive_upload_timeout_seconds: 86_400,
    }
}

/// A router built on `config`, authenticating normally (devices only, no Keycloak).
#[allow(dead_code)]
pub fn build_test_app_with_config(db: DatabaseConnection, config: Config) -> axum::Router {
    let state = AppState::new(db, config, None);
    deepreefmap_api::routes::build_router(&state)
}

/// A router built on `config` treating every request as a Keycloak login.
#[allow(dead_code)]
pub fn build_test_app_with_config_as_human(
    db: DatabaseConnection,
    config: Config,
    sub: &str,
    roles: Vec<Role>,
) -> axum::Router {
    let state = AppState::new(db, config, None);
    deepreefmap_api::routes::build_router_as(
        &state,
        AuthContext::Keycloak {
            sub: sub.to_string(),
            email: Some(format!("{sub}@test.local")),
            roles,
        },
    )
}

pub fn build_test_app(db: DatabaseConnection) -> axum::Router {
    let state = AppState::new(db, test_config(), None);
    deepreefmap_api::routes::build_router(&state)
}

/// A router that treats every request as a Keycloak login carrying `roles`.
///
/// No Keycloak runs here, so this is the only way to exercise member against admin.
#[allow(dead_code)]
pub fn build_test_app_as_human(
    db: DatabaseConnection,
    sub: &str,
    roles: Vec<Role>,
) -> axum::Router {
    let state = AppState::new(db, test_config(), None);
    deepreefmap_api::routes::build_router_as(
        &state,
        AuthContext::Keycloak {
            sub: sub.to_string(),
            email: Some(format!("{sub}@test.local")),
            roles,
        },
    )
}

#[allow(dead_code)]
pub fn build_test_app_as_admin(db: DatabaseConnection) -> axum::Router {
    build_test_app_as_human(db, "admin-sub", vec![Role::Administrator])
}

#[allow(dead_code)]
pub fn build_test_app_as_member(db: DatabaseConnection) -> axum::Router {
    build_test_app_as_human(db, "member-sub", vec![Role::Member])
}

/// A router plus the state it shares, for tests that need to reach the token cache.
// Each test binary compiles this module separately, so a helper only some of them use
// looks dead to the others.
#[allow(dead_code)]
pub fn build_test_app_with_state(db: DatabaseConnection) -> (axum::Router, AppState) {
    let state = AppState::new(db, test_config(), None);
    let app = deepreefmap_api::routes::build_router(&state);
    (app, state)
}

/// How many rows a push section acknowledged as written.
pub fn applied(section: &serde_json::Value) -> usize {
    section["applied"].as_array().map_or(0, Vec::len)
}

/// The ids under one refusal list of a push section: `superseded`, `proposed` or
/// `rejected`.
pub fn refused(section: &serde_json::Value, list: &str) -> Vec<String> {
    section[list]
        .as_array()
        .map(|rows| {
            rows.iter()
                .map(|r| r["id"].as_str().unwrap_or_default().to_string())
                .collect()
        })
        .unwrap_or_default()
}
