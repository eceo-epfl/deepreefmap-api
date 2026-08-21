//! Trading a connect code for a device token.
//!
//! Unauthenticated, since the code is the credential. Everything else about devices
//! lives behind the gate in [`crate::routes::private::devices`].

use std::time::Duration as StdDuration;

use axum::{Json, Router, extract::State, routing::post};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, Set, TransactionTrait, sea_query::Expr,
};
use tower_governor::{
    GovernorLayer, governor::GovernorConfigBuilder, key_extractor::SmartIpKeyExtractor,
};
use tower_http::limit::RequestBodyLimitLayer;
use utoipa::ToSchema;
use uuid::Uuid;

use crate::common::AppState;
use crate::common::contract::ClientContract;
use crate::common::tokens::{mint_device_token, parse_connect_code, sha256_hex};
use crate::error::{AppError, AppResult};
use crate::routes::private::devices::{connect_code, model as device};

/// Body ceiling on enrolment, which takes a code and two short strings.
const ENROL_BODY_LIMIT: usize = 8 * 1024;

/// The enrolment route with its body limit and per-IP rate limiter.
///
/// Rate limited because each attempt costs an argon2 verification.
///
/// # Panics
///
/// Panics when the rate limiter configuration is invalid, which the constants here
/// cannot produce.
pub fn router(state: &AppState) -> Router {
    let config = &state.config;
    let router = Router::new()
        .route("/enrol", post(enrol))
        .layer(RequestBodyLimitLayer::new(ENROL_BODY_LIMIT));
    let router = if config.disable_rate_limiting {
        tracing::warn!("Rate limiting DISABLED, including on enrolment");
        router
    } else {
        // Keys on the forwarded client address, since the peer is our own proxy and
        // every enrolling laptop would otherwise share one bucket.
        let limiter = GovernorConfigBuilder::default()
            .period(StdDuration::from_secs(config.enrol_rate_limit_period_secs))
            .burst_size(config.enrol_rate_limit_burst)
            .key_extractor(SmartIpKeyExtractor)
            .finish()
            .expect("enrolment rate limiter configuration is valid");
        router.layer(GovernorLayer::new(limiter))
    };
    router.with_state(state.clone())
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct EnrolRequest {
    /// The whole `drm1.…` string or its bare secret.
    pub code: String,
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
    /// This installation's durable name, chosen when its connect code was minted and
    /// shown as `uploaded_by` on what it pushes.
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

    // The name was chosen when the code was minted: one origin, in the portal. Old
    // codes carry an empty name, which falls back to something visible.
    let name = match code.device_name.trim() {
        "" => format!("Device {}", &device_id.to_string()[..8]),
        trimmed => trimmed.to_string(),
    };

    device::ActiveModel {
        id: Set(device_id),
        enrolled_by: Set(code.created_by.clone()),
        name: Set(name.clone()),
        token_prefix: Set(minted.token_prefix),
        token_hash: Set(minted.token_hash),
        platform: Set(body.platform),
        gui_version: Set(body.gui_version),
        library_version: Set(body.library_version),
        system_profile: Set(None),
        profile_reported_at: Set(None),
        preset_schema_version: Set(None),
        assigned_preset_id: Set(None),
        assigned_at: Set(None),
        active_preset_name: Set(None),
        active_preset_version: Set(None),
        active_preset_reported_at: Set(None),
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
        device_name: name,
        token: minted.raw_token,
    }))
}
