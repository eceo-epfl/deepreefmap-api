//! Applying a client's document to the registry.
//!
//! A push may insert a row in a section its origin authors, and amend only what that
//! origin already authored. Sections outside [`CLIENT_PUSH_SECTIONS`] are read, never
//! written. Within one origin, conflicts resolve last-write-wins on `updated_at`.

use axum::{Json, extract::State};
use sea_orm::{ConnectionTrait, Statement, TransactionTrait};
use std::collections::HashMap;
use utoipa::ToSchema;

use super::schema::{CLIENT_PUSH_SECTIONS, ColumnSpec, TABLES, table_for_section, to_value};
use crate::common::AppState;
use crate::common::auth::{AuthContext, Origin};
use crate::common::contract::ClientContract;
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct PushRequest {
    /// Contract version this document was built against, which must be the one negotiated
    /// for the exchange.
    pub contract_version: u32,
    /// Rows keyed by section name, as listed in `contract/sync-contract.json`.
    #[schema(value_type = Object)]
    pub sections: HashMap<String, Vec<serde_json::Value>>,
}

#[derive(Debug, Default, serde::Serialize, ToSchema)]
pub struct SectionOutcome {
    pub received: usize,
    /// Rows written, inserted or updated.
    pub applied: usize,
    /// Ids this origin owns but the server holds at an equal or newer `updated_at`; pull
    /// them to see what won.
    pub skipped: Vec<uuid::Uuid>,
    /// Ids left untouched because this origin does not author them: another origin's row,
    /// or any row of a section this origin may not write. A human reconciles these.
    pub refused: Vec<uuid::Uuid>,
    /// Ids the database would not take: a unique collision, a missing parent, or a value
    /// outside its allowed set. Re-sending the same row cannot help.
    pub conflicted: Vec<uuid::Uuid>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PushResponse {
    /// Version agreed for this exchange, which is what the document had to declare.
    pub contract_version: u32,
    /// Sequence position after this push, for the next pull's `since`.
    pub cursor: i64,
    #[schema(value_type = Object)]
    pub sections: HashMap<String, SectionOutcome>,
}

/// Apply a document of survey metadata.
///
/// One transaction, sections in foreign-key order, so a client need not order its
/// own writes.
#[utoipa::path(
    post,
    path = "/sync/push",
    request_body = PushRequest,
    responses(
        (status = 200, description = "Document applied", body = PushResponse),
        (status = 400, description = "Unknown section, bad contract version, or malformed row"),
        (status = 409, description = "A row references something that does not exist"),
    ),
    tag = "sync"
)]
pub async fn push(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    axum::Extension(contract): axum::Extension<ClientContract>,
    Json(body): Json<PushRequest>,
) -> AppResult<Json<PushResponse>> {
    // The agreed version, not this server's maximum, so a client that negotiated down to an
    // older one is still writing a document the server asked it for.
    let agreed = contract.agreed();
    if body.contract_version != agreed {
        return Err(AppError::BadRequest(format!(
            "Unsupported contract_version {}: this exchange agreed on {agreed}",
            body.contract_version
        )));
    }

    // Before writing anything: applying the rest would silently drop rows the client
    // believes it handed over.
    for section in body.sections.keys() {
        if table_for_section(section).is_none() {
            return Err(AppError::BadRequest(format!("Unknown section: {section}")));
        }
    }

    // Provenance is the pushing laptop alone. A human push binds NULL, the same
    // origin a console-authored row carries.
    let device_id = match auth.origin() {
        Origin::Human { .. } => None,
        Origin::Device { device_id, .. } => Some(device_id),
    };

    let txn = state.db.begin().await?;
    let mut outcomes: HashMap<String, SectionOutcome> = HashMap::new();

    for spec in TABLES {
        let Some(rows) = body.sections.get(spec.section) else {
            continue;
        };
        let outcome = SectionOutcome {
            received: rows.len(),
            ..Default::default()
        };

        // Read, never written: a device sends these as ancestors of what it does author.
        let outcome = if device_id.is_some() && !CLIENT_PUSH_SECTIONS.contains(&spec.section) {
            reference_only(spec, rows, outcome)?
        } else {
            apply_section(&txn, spec, rows, device_id, outcome).await?
        };

        outcomes.insert(spec.section.to_string(), outcome);
    }

    let cursor = super::current_cursor(&txn).await?;
    txn.commit().await?;

    Ok(Json(PushResponse {
        contract_version: agreed,
        cursor,
        sections: outcomes,
    }))
}

/// The row's `id`, which every section needs before anything else can be read from it.
fn row_id(section: &str, row: &serde_json::Value) -> AppResult<uuid::Uuid> {
    row.as_object()
        .and_then(|object| object.get("id"))
        .and_then(serde_json::Value::as_str)
        .and_then(|s| s.parse::<uuid::Uuid>().ok())
        .ok_or_else(|| AppError::BadRequest(format!("{section}: every row needs a UUID id")))
}

/// Upsert every row of a section the caller authors, a savepoint at a time.
async fn apply_section(
    txn: &sea_orm::DatabaseTransaction,
    spec: &super::schema::TableSpec,
    rows: &[serde_json::Value],
    device_id: Option<uuid::Uuid>,
    mut outcome: SectionOutcome,
) -> AppResult<SectionOutcome> {
    let columns = spec.columns();
    let sql = upsert_sql(spec.table, &columns);

    for row in rows {
        let object = row.as_object().ok_or_else(|| {
            AppError::BadRequest(format!("{}: each row must be an object", spec.section))
        })?;
        let id = row_id(spec.section, row)?;

        // Provenance comes from the credential, never the payload.
        let mut values = Vec::with_capacity(columns.len());
        for column in &columns {
            let value = match column.name {
                "device_id" => sea_orm::Value::Uuid(device_id),
                "updated_at" => clamp_stamp(spec.section, id, object.get(column.name))?,
                _ => to_value(column, object.get(column.name)).map_err(|e| match e {
                    AppError::BadRequest(msg) => {
                        AppError::BadRequest(format!("{} {id}: {msg}", spec.section))
                    }
                    other => other,
                })?,
            };
            values.push(value);
        }

        let statement =
            Statement::from_sql_and_values(sea_orm::DatabaseBackend::Postgres, &sql, values);

        // A savepoint per row: the client re-sends every ancestor, so a document-level
        // failure would repeat for good.
        let savepoint = txn.begin().await?;
        let written = match savepoint.query_one_raw(statement).await {
            Ok(row) => {
                let row = row.ok_or_else(|| {
                    AppError::Database(sea_orm::DbErr::Custom(format!(
                        "{} {id}: the upsert reported nothing",
                        spec.section
                    )))
                })?;
                savepoint.commit().await?;
                row
            }
            Err(e) => {
                savepoint.rollback().await?;
                if is_constraint_violation(&e) {
                    tracing::info!(
                        section = spec.section,
                        %id,
                        error = %e,
                        "Refused a row the database would not take"
                    );
                    outcome.conflicted.push(id);
                    continue;
                }
                return Err(translate_write_error(spec.section, id, e));
            }
        };

        if written.try_get::<bool>("", "written")? {
            outcome.applied += 1;
        } else if written.try_get::<bool>("", "refused")? {
            outcome.refused.push(id);
        } else {
            outcome.skipped.push(id);
        }
    }

    Ok(outcome)
}

/// Account for a section the caller may not author, reporting every row as `refused`.
///
/// A row that is absent needs no separate answer: the child referencing it fails its
/// foreign key and is named on its own section.
fn reference_only(
    spec: &super::schema::TableSpec,
    rows: &[serde_json::Value],
    mut outcome: SectionOutcome,
) -> AppResult<SectionOutcome> {
    for row in rows {
        outcome.refused.push(row_id(spec.section, row)?);
    }
    Ok(outcome)
}

/// Insert a row, or amend it only where the pushing origin authored it, and say which.
///
/// `EXCLUDED.device_id` is the pushing origin, since provenance binds from the credential
/// and never the payload, so the ownership test needs no further parameter. The outer
/// `SELECT` reads the table as it stood before the insert, which is what tells a refusal
/// apart from a stale skip.
fn upsert_sql(table: &str, columns: &[ColumnSpec]) -> String {
    // Names, placeholders and positions all come from the descriptor, so they cannot
    // disagree.
    let names: Vec<&str> = columns.iter().map(|c| c.name).collect();
    let placeholders: Vec<String> = (1..=columns.len()).map(|i| format!("${i}")).collect();
    let updates: Vec<String> = names
        .iter()
        .filter(|name| **name != "id" && **name != "created_at")
        .map(|name| format!("{name} = EXCLUDED.{name}"))
        .collect();

    format!(
        "WITH written AS ( \
           INSERT INTO {table} ({cols}) VALUES ({vals}) \
           ON CONFLICT (id) DO UPDATE SET {updates} \
             WHERE {table}.device_id IS NOT DISTINCT FROM EXCLUDED.device_id \
               AND EXCLUDED.updated_at > {table}.updated_at \
           RETURNING id \
         ) \
         SELECT EXISTS (SELECT 1 FROM written) AS written, \
                EXISTS ( \
                  SELECT 1 FROM {table} \
                  WHERE id = ${id} AND device_id IS DISTINCT FROM ${device} \
                ) AS refused",
        cols = names.join(", "),
        vals = placeholders.join(", "),
        updates = updates.join(", "),
        id = position(&names, "id"),
        device = position(&names, "device_id"),
    )
}

/// The one-based bind position of a column, for a `$n` placeholder.
///
/// # Panics
///
/// Panics when the column is absent, which the descriptor cannot produce: `id` and
/// `device_id` are on every table.
fn position(names: &[&str], column: &str) -> usize {
    names
        .iter()
        .position(|name| *name == column)
        .expect("every replicated table carries id and device_id")
        + 1
}

/// How far ahead of the server a client's clock may be before its rows are refused.
const MAX_CLOCK_SKEW: chrono::TimeDelta = chrono::TimeDelta::hours(1);

/// Bind `updated_at`, pulled back to now when the client's clock runs ahead.
///
/// Conflicts resolve on this stamp, so an unbounded one pins the row against every later
/// correction, the honest client's own included. Clamped rather than refused: a laptop with
/// a wrong clock, or one carrying a row stamped by some earlier bug, would otherwise never
/// sync again, and it re-sends every ancestor on every push.
fn clamp_stamp(
    section: &str,
    id: uuid::Uuid,
    raw: Option<&serde_json::Value>,
) -> AppResult<sea_orm::Value> {
    let spec = ColumnSpec {
        name: "updated_at",
        kind: crate::routes::private::sync::schema::ColumnKind::Timestamp,
        nullable: false,
    };
    let value = to_value(&spec, raw).map_err(|e| match e {
        AppError::BadRequest(msg) => AppError::BadRequest(format!("{section} {id}: {msg}")),
        other => other,
    })?;

    let now = chrono::Utc::now();
    let ceiling = now + MAX_CLOCK_SKEW;
    if let sea_orm::Value::ChronoDateTimeUtc(Some(stamp)) = &value
        && *stamp > ceiling
    {
        tracing::warn!(
            section,
            %id,
            stamp = %stamp,
            "Clamped an updated_at from the future: check the client's clock"
        );
        return Ok(sea_orm::Value::from(now));
    }
    Ok(value)
}

/// Whether a write failed on a unique, foreign key or check constraint.
fn is_constraint_violation(error: &sea_orm::DbErr) -> bool {
    let text = error.to_string();
    text.contains("violates unique constraint")
        || text.contains("violates foreign key constraint")
        || text.contains("violates check constraint")
}

/// Turn a constraint violation into something the client can act on.
fn translate_write_error(section: &str, id: uuid::Uuid, error: sea_orm::DbErr) -> AppError {
    let text = error.to_string();
    if text.contains("violates foreign key constraint") {
        AppError::Conflict(format!(
            "{section} {id} references a row this server does not have; \
             push its parents in the same document"
        ))
    } else if text.contains("violates unique constraint") {
        AppError::Conflict(format!(
            "{section} {id} collides with an existing row on a unique field"
        ))
    } else if text.contains("violates check constraint") {
        AppError::BadRequest(format!(
            "{section} {id} has a value outside its allowed set"
        ))
    } else {
        AppError::Database(error)
    }
}
