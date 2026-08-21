//! A device's report of what it runs, sent under its own credential.
//!
//! Enrolment captures versions once, then laptops update in the field. The heartbeat
//! keeps the registry current without a person transcribing version numbers, and it is
//! how a preset assignment travels: the response names the server-chosen default, the
//! next request reports the preset actually in use, and the gap between the two is
//! what the console shows as "not yet acknowledged".

use axum::{Json, extract::State};
use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set};
use utoipa::ToSchema;
use uuid::Uuid;

use super::model as device;
use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::error::{AppError, AppResult};
use crate::routes::private::presets;

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
    /// Which `preset-schema.json` revision this installation understands.
    #[serde(default)]
    pub preset_schema_version: Option<i32>,
    /// The preset the device currently runs under, acknowledging an assignment.
    #[serde(default)]
    pub active_preset_name: Option<String>,
    #[serde(default)]
    pub active_preset_version: Option<i32>,
}

/// The server-chosen default preset, named well enough to select locally.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct AssignedPreset {
    pub id: Uuid,
    pub name: String,
    pub version: i32,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct HeartbeatResponse {
    /// Null when nothing is assigned, or the assigned preset has been deleted.
    pub assigned_preset: Option<AssignedPreset>,
}

/// Update the calling device's own record and learn its assigned preset.
///
/// Identity comes from the credential and never from the body, so a device cannot
/// report on a sibling's behalf.
#[utoipa::path(
    post,
    path = "/sync/heartbeat",
    request_body = HeartbeatRequest,
    responses(
        (status = 200, description = "Report stored; the response names the assigned preset", body = HeartbeatResponse),
        (status = 400, description = "system_profile is not a JSON object"),
        (status = 403, description = "Requires a device token"),
    ),
    tag = "devices"
)]
pub async fn heartbeat(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<HeartbeatRequest>,
) -> AppResult<Json<HeartbeatResponse>> {
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
    if body.preset_schema_version.is_some() {
        update.preset_schema_version = Set(body.preset_schema_version);
    }
    if body.active_preset_name.is_some() || body.active_preset_version.is_some() {
        update.active_preset_name = Set(body.active_preset_name);
        update.active_preset_version = Set(body.active_preset_version);
        update.active_preset_reported_at = Set(Some(Utc::now()));
    }
    update.update(&state.db).await?;

    let row = device::Entity::find_by_id(device_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;
    let assigned_preset = match row.assigned_preset_id {
        None => None,
        Some(preset_id) => presets::Entity::find_by_id(preset_id)
            .filter(presets::Column::DeletedAt.is_null())
            .one(&state.db)
            .await?
            .map(|preset| AssignedPreset {
                id: preset.id,
                name: preset.name,
                version: preset.version,
            }),
    };

    Ok(Json(HeartbeatResponse { assigned_preset }))
}
