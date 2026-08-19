//! Minting connect codes, and trading one for a device token.
//!
//! Lets the desktop application ship with no server address and no realm details in
//! it: onboarding is a single paste, and self-hosted servers work the same way.

use axum::{Json, extract::State};
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, ExprTrait, QueryFilter, Set, TransactionTrait,
    sea_query::Expr,
};
use utoipa::ToSchema;
use uuid::Uuid;

use super::{connect_code, model as device};
use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::common::contract::ClientContract;
use crate::common::tokens::{mint_connect_code, mint_device_token, parse_connect_code, sha256_hex};
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct MintConnectCodeRequest {
    /// Label for the unredeemed code, so the operator knows who they handed it to.
    /// Never attribution: the device names itself at enrolment.
    #[serde(default)]
    pub note: String,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct MintConnectCodeResponse {
    /// The string to paste into the desktop application. Shown once.
    pub code: String,
    pub expires_at: chrono::DateTime<chrono::Utc>,
}

/// Mint a connect code for a desktop installation.
///
/// Interactive login only, so a device cannot invite further devices.
#[utoipa::path(
    post,
    path = "/devices/connect-codes",
    request_body = MintConnectCodeRequest,
    responses(
        (status = 200, description = "Code minted; shown to the operator once", body = MintConnectCodeResponse),
        (status = 403, description = "Requires an interactive login"),
        (status = 500, description = "PUBLIC_BASE_URL is not configured"),
    ),
    tag = "devices"
)]
pub async fn mint_code(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<MintConnectCodeRequest>,
) -> AppResult<Json<MintConnectCodeResponse>> {
    // Behind require_human, so this is belt and braces rather than a live path.
    let operator = auth
        .human_subject()
        .ok_or_else(|| AppError::Forbidden("Devices cannot mint connect codes".to_string()))?;

    // A code with no address cannot be pasted anywhere, and would look like it worked.
    let base_url = state.config.public_base_url.as_ref().ok_or_else(|| {
        AppError::Internal("PUBLIC_BASE_URL must be set to mint connect codes".to_string())
    })?;

    // Retention housekeeping lives here rather than on a scheduler: codes are only
    // ever created here, so growth cannot outpace the cleanup, and the registry needs
    // no extra timer task. Codes never sync, so a hard delete resurrects nothing.
    let retention_cutoff = Utc::now() - Duration::days(90);
    connect_code::Entity::delete_many()
        .filter(
            sea_orm::Condition::any()
                .add(connect_code::Column::UsedAt.lt(retention_cutoff))
                .add(
                    connect_code::Column::UsedAt
                        .is_null()
                        .and(connect_code::Column::ExpiresAt.lt(retention_cutoff)),
                ),
        )
        .exec(&state.db)
        .await?;

    let minted = mint_connect_code(base_url);
    let expires_at = Utc::now() + Duration::seconds(state.config.connect_code_ttl_seconds);

    connect_code::ActiveModel {
        id: Set(Uuid::new_v4()),
        code_hash: Set(minted.code_hash),
        created_by: Set(Some(operator.to_string())),
        note: Set(body.note),
        expires_at: Set(expires_at),
        used_at: Set(None),
        used_by_device_id: Set(None),
        created_at: Set(Utc::now()),
    }
    .insert(&state.db)
    .await?;

    Ok(Json(MintConnectCodeResponse {
        code: minted.code,
        expires_at,
    }))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct EnrolRequest {
    /// The whole `drm1.…` string or its bare secret.
    pub code: String,
    /// This installation's durable name, shown as `uploaded_by` on what it pushes.
    pub device_name: String,
    #[serde(default)]
    pub platform: Option<String>,
    #[serde(default)]
    pub gui_version: Option<String>,
    #[serde(default)]
    pub library_version: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct EnrolResponse {
    /// Version agreed for this exchange, so a fresh installation learns it before its
    /// first push.
    pub contract_version: u32,
    pub device_id: Uuid,
    /// The name accepted for this installation.
    pub device_name: String,
    /// Bearer token for every later sync request. Returned once; only its hash is kept.
    pub token: String,
}

/// Trade a connect code for a long-lived device token.
///
/// Unauthenticated, since the code is the credential: rate limited per IP, and the
/// code is spent in the same transaction that creates the device.
#[utoipa::path(
    post,
    path = "/enrol",
    request_body = EnrolRequest,
    responses(
        (status = 200, description = "Device enrolled", body = EnrolResponse),
        (status = 400, description = "Malformed request"),
        (status = 401, description = "Code unknown, expired, or already used"),
    ),
    tag = "devices"
)]
pub async fn enrol(
    State(state): State<AppState>,
    axum::Extension(contract): axum::Extension<ClientContract>,
    Json(body): Json<EnrolRequest>,
) -> AppResult<Json<EnrolResponse>> {
    let name = body.device_name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest(
            "device_name must not be empty".to_string(),
        ));
    }

    let secret = parse_connect_code(&body.code)
        .ok_or_else(|| AppError::Unauthorized("Invalid connect code".to_string()))?;
    let code_hash = sha256_hex(&secret);

    let txn = state.db.begin().await?;

    let code = connect_code::Entity::find()
        .filter(connect_code::Column::CodeHash.eq(&code_hash))
        .filter(connect_code::Column::UsedAt.is_null())
        .filter(connect_code::Column::ExpiresAt.gt(Utc::now()))
        .one(&txn)
        .await?
        // One message for unknown, expired and spent alike.
        .ok_or_else(|| AppError::Unauthorized("Invalid or expired connect code".to_string()))?;

    // Spent before minting, so the row lock covers the argon2 hash rather than sitting
    // beside it. A racing enrolment blocks here, then matches nothing.
    let spent = connect_code::Entity::update_many()
        .col_expr(connect_code::Column::UsedAt, Expr::value(Some(Utc::now())))
        .filter(connect_code::Column::Id.eq(code.id))
        .filter(connect_code::Column::UsedAt.is_null())
        .exec(&txn)
        .await?;
    if spent.rows_affected != 1 {
        return Err(AppError::Unauthorized(
            "Invalid or expired connect code".to_string(),
        ));
    }

    let minted = mint_device_token();
    let device_id = Uuid::new_v4();

    device::ActiveModel {
        id: Set(device_id),
        enrolled_by: Set(code.created_by.clone()),
        name: Set(name.to_string()),
        token_prefix: Set(minted.token_prefix),
        token_hash: Set(minted.token_hash),
        platform: Set(body.platform),
        gui_version: Set(body.gui_version),
        library_version: Set(body.library_version),
        system_profile: Set(None),
        profile_reported_at: Set(None),
        created_at: Set(Utc::now()),
        last_seen_at: Set(None),
        revoked_at: Set(None),
    }
    .insert(&txn)
    .await?;

    // Once the device exists: the foreign key is immediate.
    connect_code::Entity::update_many()
        .col_expr(
            connect_code::Column::UsedByDeviceId,
            Expr::value(Some(device_id)),
        )
        .filter(connect_code::Column::Id.eq(code.id))
        .exec(&txn)
        .await?;

    txn.commit().await?;

    tracing::info!(%device_id, enrolled_by = ?code.created_by, "Device enrolled");

    Ok(Json(EnrolResponse {
        contract_version: contract.agreed(),
        device_id,
        device_name: name.to_string(),
        token: minted.raw_token,
    }))
}

/// Gate the two operations a device must never perform on itself.
///
/// `enrolled_by` is audit information everywhere else, and this is the one place it is
/// read: the person who onboarded a laptop keeps a way to retire and relabel it.
fn authorise_device_admin(auth: &AuthContext, found: &device::Model) -> Result<(), AppError> {
    let sub = auth
        .human_subject()
        .ok_or_else(|| AppError::Forbidden("Devices cannot administer devices".to_string()))?;
    if found.enrolled_by.as_deref() == Some(sub) || auth.is_admin() {
        return Ok(());
    }
    Err(AppError::Forbidden("Not your device".to_string()))
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RevokeResponse {
    pub device_id: Uuid,
    pub revoked_at: chrono::DateTime<chrono::Utc>,
}

/// Revoke a device. Members may revoke their own, administrators anyone's.
#[utoipa::path(
    post,
    path = "/devices/{device_id}/revoke",
    params(("device_id" = Uuid, Path, description = "Device to revoke")),
    responses(
        (status = 200, description = "Device revoked", body = RevokeResponse),
        (status = 403, description = "Not your device"),
        (status = 404, description = "No such device"),
    ),
    tag = "devices"
)]
pub async fn revoke(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    axum::extract::Path(device_id): axum::extract::Path<Uuid>,
) -> AppResult<Json<RevokeResponse>> {
    let found = device::Entity::find_by_id(device_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;

    authorise_device_admin(&auth, &found)?;

    let revoked_at = found.revoked_at.unwrap_or_else(Utc::now);
    if found.revoked_at.is_none() {
        let mut update: device::ActiveModel = found.into();
        update.revoked_at = Set(Some(revoked_at));
        update.update(&state.db).await?;
        // Keyed by raw token, unrecoverable here, so the whole bounded cache goes.
        state.device_token_cache.invalidate_all();
    }

    tracing::info!(%device_id, "Device revoked");
    Ok(Json(RevokeResponse {
        device_id,
        revoked_at,
    }))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct RenameDeviceRequest {
    pub name: String,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RenameDeviceResponse {
    pub device_id: Uuid,
    pub name: String,
}

/// Rename a device. Members may rename their own, administrators anyone's.
///
/// Interactive login only. A device that could rename itself would make its own
/// attribution editable.
#[utoipa::path(
    post,
    path = "/devices/{device_id}/rename",
    params(("device_id" = Uuid, Path, description = "Device to rename")),
    request_body = RenameDeviceRequest,
    responses(
        (status = 200, description = "Device renamed", body = RenameDeviceResponse),
        (status = 400, description = "Empty name"),
        (status = 403, description = "Not your device, or a device token"),
        (status = 404, description = "No such device"),
    ),
    tag = "devices"
)]
pub async fn rename(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    axum::extract::Path(device_id): axum::extract::Path<Uuid>,
    Json(body): Json<RenameDeviceRequest>,
) -> AppResult<Json<RenameDeviceResponse>> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(AppError::BadRequest("name must not be empty".to_string()));
    }

    let found = device::Entity::find_by_id(device_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Device not found".to_string()))?;

    authorise_device_admin(&auth, &found)?;

    let mut update: device::ActiveModel = found.into();
    update.name = Set(name.to_string());
    update.update(&state.db).await?;
    // The cached model carries the old name, which /api/me would keep reporting.
    state.device_token_cache.invalidate_all();

    tracing::info!(%device_id, %name, "Device renamed");
    Ok(Json(RenameDeviceResponse {
        device_id,
        name: name.to_string(),
    }))
}
