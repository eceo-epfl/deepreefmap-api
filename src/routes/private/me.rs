use axum::Json;
use utoipa::ToSchema;

use crate::common::auth::{AuthContext, Origin};
use crate::error::AppResult;

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
