//! Publishing a calibration a laptop made.
//!
//! Devices do not author camera rows through the change ledger: a profile is
//! published so every laptop rectifies the same footage the same way, which is a
//! console decision, and a curated push would put a mid-expedition recalibration in a
//! review queue nobody at sea can reach. This is the archive's shape instead, a plain
//! endpoint under the device's own credential, and what it writes is a row like any
//! other the console can then assign.
//!
//! Idempotent by content: the same document under the same profile name returns the
//! calibration already stored. A different document takes the next version rather than
//! overwriting one, because the calibration it would replace is what some run was
//! rectified with.

use axum::{Json, extract::State};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, Set, TransactionTrait,
};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{calibration, model as camera_profile};
use crate::common::AppState;
use crate::common::auth::{AuthContext, Origin};
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct UploadRequest {
    /// The profile this measures. Made if the registry has never seen it.
    pub name: String,
    /// The profile JSON the desktop writes, verbatim.
    pub document: serde_json::Value,
    #[serde(default)]
    pub source_clip: Option<String>,
    #[serde(default)]
    pub reprojection_error_px: Option<f64>,
    #[serde(default)]
    pub registered_frames: Option<i32>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct UploadResponse {
    pub camera_profile_id: Uuid,
    pub camera_calibration_id: Uuid,
    pub version: i32,
    /// False when this document was already the profile's calibration at this version.
    pub created: bool,
}

/// The image size the document declares, where it declares one.
fn image_size(document: &serde_json::Value) -> (Option<i32>, Option<i32>) {
    let size = document
        .get("rectified_pinhole")
        .and_then(|r| r.get("image_size"))
        .and_then(|s| s.as_array());
    let read = |index: usize| {
        size.and_then(|s| s.get(index))
            .and_then(serde_json::Value::as_i64)
            .and_then(|v| i32::try_from(v).ok())
    };
    (read(0), read(1))
}

/// Publish a calibration made on a laptop.
#[utoipa::path(
    post,
    path = "/upload",
    request_body = UploadRequest,
    responses((status = 200, body = UploadResponse)),
    tag = "camera_calibrations",
)]
pub async fn upload(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<UploadRequest>,
) -> AppResult<Json<UploadResponse>> {
    if !camera_profile::name_is_resolvable(&body.name) {
        return Err(AppError::BadRequest(format!(
            "{:?} is not a profile name a device can resolve: letters, numbers, \
             underscores and hyphens only",
            body.name
        )));
    }
    calibration::validate_document(&body.document).map_err(AppError::BadRequest)?;
    let device_id = match auth.origin() {
        Origin::Device { device_id, .. } => Some(device_id),
        Origin::Human { .. } => None,
    };

    let txn = state.db.begin().await?;

    // One profile per name, made on first sight so a laptop can publish without a
    // curator having named the rig first.
    let existing = camera_profile::Entity::find()
        .filter(camera_profile::Column::Name.eq(body.name.clone()))
        .filter(camera_profile::Column::DeletedAt.is_null())
        .one(&txn)
        .await?;
    let profile_id = if let Some(profile) = existing {
        profile.id
    } else {
        camera_profile::ActiveModel {
            id: Set(Uuid::new_v4()),
            name: Set(body.name.clone()),
            description: Set(String::new()),
            device_id: Set(device_id),
            ..Default::default()
        }
        .insert(&txn)
        .await?
        .id
    };

    let held = calibration::Entity::find()
        .filter(calibration::Column::CameraProfileId.eq(profile_id))
        .filter(calibration::Column::DeletedAt.is_null())
        .order_by_desc(calibration::Column::Version)
        .all(&txn)
        .await?;
    if let Some(same) = held.iter().find(|row| row.document == body.document) {
        txn.commit().await?;
        return Ok(Json(UploadResponse {
            camera_profile_id: profile_id,
            camera_calibration_id: same.id,
            version: same.version,
            created: false,
        }));
    }

    let (width, height) = image_size(&body.document);
    let version = held.first().map_or(1, |row| row.version + 1);
    let stored = calibration::ActiveModel {
        id: Set(Uuid::new_v4()),
        camera_profile_id: Set(profile_id),
        version: Set(version),
        document: Set(body.document),
        image_width: Set(width),
        image_height: Set(height),
        reprojection_error_px: Set(body.reprojection_error_px),
        registered_frames: Set(body.registered_frames),
        source_clip: Set(body.source_clip.unwrap_or_default()),
        calibrated_at: Set(None),
        description: Set(body.description.unwrap_or_default()),
        device_id: Set(device_id),
        ..Default::default()
    }
    .insert(&txn)
    .await?;
    txn.commit().await?;

    Ok(Json(UploadResponse {
        camera_profile_id: profile_id,
        camera_calibration_id: stored.id,
        version: stored.version,
        created: true,
    }))
}
