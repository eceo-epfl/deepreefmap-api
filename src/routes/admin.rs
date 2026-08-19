//! Administrative erasure of a person's identifiers.
//!
//! `device.enrolled_by`, `connect_code.created_by` and `stored_object.uploaded_by`
//! carry Keycloak subjects, which are personal data. Erasure nulls them wherever the
//! subject appears. Survey rows never name a person: their provenance is the device
//! that pushed them, so they need no scrubbing.

use axum::{Json, extract::State};
use sea_orm::{ConnectionTrait, Statement};
use std::collections::BTreeMap;
use utoipa::ToSchema;

use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct EraseSubjectRequest {
    /// The Keycloak subject to scrub.
    pub subject: String,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct EraseSubjectResponse {
    /// Rows scrubbed, per table.
    pub scrubbed: BTreeMap<String, u64>,
}

/// Null every reference to a Keycloak subject. Administrators only.
///
/// Touches only the onboarding and upload audit columns. None of them sync, so the
/// erasure is complete on the server and nothing propagates to devices.
#[utoipa::path(
    post,
    path = "/admin/erase-subject",
    request_body = EraseSubjectRequest,
    responses(
        (status = 200, description = "Rows scrubbed per table", body = EraseSubjectResponse),
        (status = 400, description = "Empty subject"),
        (status = 403, description = "Requires the deepreefmap-admin role"),
    ),
    tag = "admin"
)]
pub async fn erase_subject(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<EraseSubjectRequest>,
) -> AppResult<Json<EraseSubjectResponse>> {
    if !auth.is_admin() {
        return Err(AppError::Forbidden(
            "Erasing a subject requires the deepreefmap-admin role".to_string(),
        ));
    }
    let subject = body.subject.trim();
    if subject.is_empty() {
        return Err(AppError::BadRequest(
            "subject must not be empty".to_string(),
        ));
    }

    let mut scrubbed = BTreeMap::new();
    for (table, sql) in [
        (
            "device",
            "UPDATE device SET enrolled_by = NULL WHERE enrolled_by = $1",
        ),
        (
            "connect_code",
            "UPDATE connect_code SET created_by = NULL WHERE created_by = $1",
        ),
        (
            "stored_object",
            "UPDATE stored_object SET uploaded_by = NULL WHERE uploaded_by = $1",
        ),
    ] {
        scrubbed.insert(
            table.to_string(),
            scrub(&state.db, table, sql, subject).await?,
        );
    }

    tracing::info!(?scrubbed, "Erased a subject");
    Ok(Json(EraseSubjectResponse { scrubbed }))
}

async fn scrub<C: ConnectionTrait>(
    db: &C,
    table: &str,
    sql: &str,
    subject: &str,
) -> AppResult<u64> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            [subject.into()],
        ))
        .await?;
    tracing::debug!(table, rows = result.rows_affected(), "Scrubbed");
    Ok(result.rows_affected())
}
