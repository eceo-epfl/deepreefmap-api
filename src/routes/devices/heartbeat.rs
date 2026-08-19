//! A device's report of what it runs, sent under its own credential.
//!
//! Enrolment captures versions once, then laptops update in the field. The heartbeat
//! keeps the registry current without a person transcribing version numbers.

use axum::{Json, extract::State, http::StatusCode};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, Set};
use utoipa::ToSchema;

use super::model as device;
use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct HeartbeatRequest {
    #[serde(default)]
    pub gui_version: Option<String>,
    #[serde(default)]
    pub library_version: Option<String>,
    #[serde(default)]
    pub platform: Option<String>,
    /// Arbitrary JSON object describing the hardware, stored as sent.
    #[serde(default)]
    pub system_profile: Option<serde_json::Value>,
}

/// Update the calling device's own record.
///
/// Identity comes from the credential and never from the body, so a device cannot
/// report on a sibling's behalf.
#[utoipa::path(
    post,
    path = "/sync/heartbeat",
    request_body = HeartbeatRequest,
    responses(
        (status = 204, description = "Report stored against the calling device"),
        (status = 400, description = "system_profile is not a JSON object"),
        (status = 403, description = "Requires a device token"),
    ),
    tag = "devices"
)]
pub async fn heartbeat(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<HeartbeatRequest>,
) -> AppResult<StatusCode> {
    // Behind require_device, so this is belt and braces rather than a live path.
    let AuthContext::Device { device_id, .. } = auth else {
        return Err(AppError::Forbidden(
            "Heartbeat is for enrolled devices".to_string(),
        ));
    };

    if let Some(profile) = &body.system_profile
        && !profile.is_object()
    {
        return Err(AppError::BadRequest(
            "system_profile must be a JSON object".to_string(),
        ));
    }

    let mut update = device::ActiveModel {
        id: Set(device_id),
        profile_reported_at: Set(Some(Utc::now())),
        ..Default::default()
    };
    // Absent fields keep their stored value, so a partial report erases nothing.
    if body.gui_version.is_some() {
        update.gui_version = Set(body.gui_version);
    }
    if body.library_version.is_some() {
        update.library_version = Set(body.library_version);
    }
    if body.platform.is_some() {
        update.platform = Set(body.platform);
    }
    if body.system_profile.is_some() {
        update.system_profile = Set(body.system_profile);
    }
    update.update(&state.db).await?;

    Ok(StatusCode::NO_CONTENT)
}
