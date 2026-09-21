//! Runtime bootstrap for the web interface.
//!
//! Realm details are fetched, not baked in, so one bundle serves every deployment.
//! Unauthenticated: a client needs this before it can authenticate, and a public
//! client id and realm name are what a browser is meant to see.

use axum::{Json, extract::State};
use utoipa::ToSchema;

use crate::common::AppState;
use crate::error::AppResult;

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct KeycloakConfigResponse {
    pub url: Option<String>,
    pub realm: Option<String>,
    /// Camel case so the object drops straight into a `keycloak-js` constructor.
    #[serde(rename = "clientId")]
    pub client_id: Option<String>,
    /// False when no realm is configured, which only local and dev may do.
    pub enabled: bool,
    /// Drives the environment banner, so nobody edits stage thinking it is local.
    pub deployment: String,
}

/// Keycloak details for the web interface.
#[utoipa::path(
    get,
    path = "/config/keycloak",
    responses((status = 200, description = "Realm details", body = KeycloakConfigResponse)),
    tag = "config"
)]
pub async fn get_keycloak_config(
    State(state): State<AppState>,
) -> AppResult<Json<KeycloakConfigResponse>> {
    let config = &state.config;
    Ok(Json(KeycloakConfigResponse {
        // The browser-facing address when one is configured, since that is who asks.
        url: config
            .keycloak_browser_url
            .clone()
            .or_else(|| config.keycloak_url.clone()),
        realm: config.keycloak_realm.clone(),
        client_id: config.keycloak_client_id.clone(),
        enabled: config.keycloak_url.is_some() && config.keycloak_realm.is_some(),
        deployment: config.deployment.as_str().to_string(),
    }))
}
