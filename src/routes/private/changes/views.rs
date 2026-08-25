//! Deciding proposals, and validating rows, from the console.

use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::{ConnectionTrait, Statement, TransactionTrait, Value};
use utoipa::ToSchema;

use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::common::ledger::validate_row;
use crate::error::{AppError, AppResult};
use crate::routes::private::sync::ledger::{project, record_console};
use crate::routes::private::sync::rows::current_image;
use crate::routes::private::sync::schema::{CONTRACT_VERSION, table_for_section};

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct DecisionResponse {
    pub seq: i64,
    pub status: String,
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct ValidateRequest {
    /// The section, as `contract/sync-contract.json` names it.
    pub section: String,
    pub ids: Vec<uuid::Uuid>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ValidateResponse {
    /// Rows this call validated. Rows already validated, or absent, are not listed.
    pub validated: Vec<uuid::Uuid>,
}

struct Proposal {
    table_key: String,
    row_id: uuid::Uuid,
    patch: serde_json::Value,
}

async fn proposal<C: ConnectionTrait>(db: &C, seq: i64) -> AppResult<Proposal> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT table_key, row_id, patch, status FROM change_log WHERE seq = $1",
            [Value::from(seq)],
        ))
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No change {seq}")))?;
    let status: String = row.try_get("", "status")?;
    if status != "proposed" {
        return Err(AppError::Conflict(format!(
            "Change {seq} is {status}, not proposed"
        )));
    }
    Ok(Proposal {
        table_key: row.try_get("", "table_key")?,
        row_id: row.try_get("", "row_id")?,
        patch: row.try_get("", "patch")?,
    })
}

async fn decide<C: ConnectionTrait>(
    db: &C,
    seq: i64,
    status: &str,
    subject: Option<&str>,
    projected_seq: Option<i64>,
) -> AppResult<()> {
    db.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE change_log SET status = $2, decided_at = NOW(), decided_by = $3, \
         decided_seq = nextval('sync_seq'), projected_seq = COALESCE($4, projected_seq) \
         WHERE seq = $1",
        [
            Value::from(seq),
            Value::from(status),
            Value::String(subject.map(str::to_string)),
            Value::BigInt(projected_seq),
        ],
    ))
    .await?;
    Ok(())
}

/// Accept a proposal: its fields land on the row as a console write.
#[utoipa::path(
    post,
    path = "/changes/{seq}/accept",
    params(("seq" = i64, Path, description = "Ledger position of the proposal")),
    responses(
        (status = 200, description = "Proposal applied", body = DecisionResponse),
        (status = 404, description = "No such change"),
        (status = 409, description = "Not a proposal"),
    ),
    tag = "changes"
)]
pub async fn accept(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Path(seq): Path<i64>,
) -> AppResult<Json<DecisionResponse>> {
    let subject = auth.human_subject().map(str::to_string);
    let txn = state.db.begin().await?;
    let proposal = proposal(&txn, seq).await?;
    let spec = table_for_section(&proposal.table_key)
        .ok_or_else(|| AppError::Internal(format!("{} is not a section", proposal.table_key)))?;

    let mut image = current_image(&txn, spec, proposal.row_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("Row {} is gone", proposal.row_id)))?;
    if let serde_json::Value::Object(patch) = proposal.patch {
        for (name, value) in patch {
            image.insert(name, value);
        }
    }
    let projected = project(&txn, spec, &spec.writable_at(CONTRACT_VERSION), &image).await?;
    validate_row(&txn, spec, proposal.row_id, subject.as_deref())
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    record_console(&txn, spec, proposal.row_id, subject.clone(), true).await?;
    decide(&txn, seq, "applied", subject.as_deref(), Some(projected)).await?;
    txn.commit().await?;

    Ok(Json(DecisionResponse {
        seq,
        status: "applied".to_string(),
    }))
}

/// Dismiss a proposal: the row stands, the values stay in the ledger.
#[utoipa::path(
    post,
    path = "/changes/{seq}/dismiss",
    params(("seq" = i64, Path, description = "Ledger position of the proposal")),
    responses(
        (status = 200, description = "Proposal dismissed", body = DecisionResponse),
        (status = 404, description = "No such change"),
        (status = 409, description = "Not a proposal"),
    ),
    tag = "changes"
)]
pub async fn dismiss(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Path(seq): Path<i64>,
) -> AppResult<Json<DecisionResponse>> {
    let txn = state.db.begin().await?;
    proposal(&txn, seq).await?;
    decide(&txn, seq, "dismissed", auth.human_subject(), None).await?;
    txn.commit().await?;
    Ok(Json(DecisionResponse {
        seq,
        status: "dismissed".to_string(),
    }))
}

/// Validate rows: from here on a laptop's change to them is a proposal.
#[utoipa::path(
    post,
    path = "/changes/validate",
    request_body = ValidateRequest,
    responses(
        (status = 200, description = "Rows validated", body = ValidateResponse),
        (status = 400, description = "Unknown section, or a transect without a site"),
    ),
    tag = "changes"
)]
pub async fn validate(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<ValidateRequest>,
) -> AppResult<Json<ValidateResponse>> {
    let spec = table_for_section(&body.section)
        .ok_or_else(|| AppError::BadRequest(format!("Unknown section: {}", body.section)))?;
    if !spec.curated {
        return Err(AppError::BadRequest(format!(
            "{} is not validated",
            body.section
        )));
    }
    let subject = auth.human_subject().map(str::to_string);
    let txn = state.db.begin().await?;
    let mut validated = Vec::with_capacity(body.ids.len());
    for id in body.ids {
        let Some(image) = current_image(&txn, spec, id).await? else {
            continue;
        };
        if spec.section == "transects"
            && image.get("site_id").is_none_or(serde_json::Value::is_null)
        {
            return Err(AppError::BadRequest(format!(
                "Transect {id} has no site; assign one before validating"
            )));
        }
        let stamped = validate_row(&txn, spec, id, subject.as_deref())
            .await
            .map_err(|e| AppError::Internal(e.to_string()))?;
        if !stamped {
            continue;
        }
        record_console(&txn, spec, id, subject.clone(), true).await?;
        validated.push(id);
    }
    txn.commit().await?;
    Ok(Json(ValidateResponse { validated }))
}
