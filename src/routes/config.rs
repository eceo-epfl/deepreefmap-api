//! Runtime bootstrap for the web interface.
//!
//! Realm details are fetched, not baked in, so one bundle serves every deployment.
//! Unauthenticated: a client needs this before it can authenticate, and a public
//! client id and realm name are what a browser is meant to see.

use axum::{Json, extract::State};
use utoipa::ToSchema;

use crate::common::AppState;
use crate::common::auth::{AuthContext, Origin};
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

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct MeResponse {
    /// Keycloak subject. Null for a device, which is an application and not a person.
    pub sub: Option<String>,
    pub email: Option<String>,
    pub is_admin: bool,
    /// True when the caller is a desktop installation rather than a person.
    pub is_device: bool,
    pub device_id: Option<uuid::Uuid>,
    /// The installation's name, which its pushed rows are attributed to.
    pub device_name: Option<String>,
}

/// Who the caller is, by whichever credential they presented.
#[utoipa::path(
    get,
    path = "/me",
    responses(
        (status = 200, description = "Caller identity", body = MeResponse),
        (status = 401, description = "No valid credential"),
    ),
    tag = "config"
)]
pub async fn get_me(
    axum::Extension(auth): axum::Extension<AuthContext>,
) -> AppResult<Json<MeResponse>> {
    let email = match &auth {
        AuthContext::Keycloak { email, .. } => email.clone(),
        AuthContext::Device { .. } => None,
    };
    let (sub, device_id, device_name) = match auth.origin() {
        Origin::Human { sub } => (Some(sub.to_string()), None, None),
        Origin::Device { device_id, name } => (None, Some(device_id), Some(name.to_string())),
    };

    Ok(Json(MeResponse {
        sub,
        email,
        is_admin: auth.is_admin(),
        is_device: device_id.is_some(),
        device_id,
        device_name,
    }))
}

/// The benthic class groups and the colour each is drawn in.
///
/// Served so the console colours a cover figure the same way the desktop viewer does.
#[utoipa::path(
    get,
    path = "/config/class-groups",
    responses((status = 200, description = "Class groups by level", body = [crate::contract::classes::ClassGroup])),
    tag = "config"
)]
pub async fn get_class_groups() -> Json<&'static [crate::contract::classes::ClassGroup]> {
    Json(crate::contract::classes::CLASS_GROUPS)
}
