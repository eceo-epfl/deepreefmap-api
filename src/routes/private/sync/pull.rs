//! Handing a client everything it has not seen.
//!
//! The cursor is a `server_seq` high-water mark over one sequence shared by every
//! replicated table, so a client tracks a single scalar. Deletes travel as rows with
//! `deleted_at` set, which is the only way they can propagate at all.
//!
//! A device pulls only [`CLIENT_PULL_SECTIONS`]. The rest is upload only.
//!
//! A client may narrow further with `Deepreefmap-Sections`, and what that narrowing drops
//! comes back in `omitted_sections`.

use axum::{
    Json,
    extract::{Query, State},
};
use sea_orm::{ConnectionTrait, Statement, TryGetable};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};

use super::schema::{CLIENT_PULL_SECTIONS, ColumnKind, ColumnSpec, TABLES, TableSpec};
use crate::common::AppState;
use crate::common::auth::AuthContext;
use crate::common::contract::ClientContract;
use crate::error::AppResult;

/// Ceiling on rows per pull. A client that hits it pulls again from the cursor.
const MAX_LIMIT: u64 = 5_000;
const DEFAULT_LIMIT: u64 = 1_000;

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct PullParams {
    /// Cursor from the previous pull or push. Omit for a full download.
    #[serde(default)]
    pub since: Option<i64>,
    /// Maximum rows across all sections, capped at 5000.
    #[serde(default)]
    pub limit: Option<u64>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PullResponse {
    pub contract_version: u32,
    /// Feed this back as `since` next time.
    pub cursor: i64,
    /// Whether rows wait beyond `cursor`. Keep pulling while true.
    pub has_more: bool,
    #[schema(value_type = Object)]
    pub sections: HashMap<String, Vec<serde_json::Value>>,
    /// Sections this build cannot read, held back by the client's own
    /// `Deepreefmap-Sections`. Upload-only sections are absent by design and not listed.
    pub omitted_sections: Vec<String>,
}

/// Download rows changed since a cursor.
#[utoipa::path(
    get,
    path = "/sync/pull",
    params(PullParams),
    responses((status = 200, description = "Changed rows in foreign-key order", body = PullResponse)),
    tag = "sync"
)]
pub async fn pull(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    axum::Extension(contract): axum::Extension<ClientContract>,
    Query(params): Query<PullParams>,
) -> AppResult<Json<PullResponse>> {
    let since = params.since.unwrap_or(0);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
    // An operator reading the registry gets the lot. A device gets only what sync
    // sends downwards.
    let device = matches!(auth, AuthContext::Device { .. });

    let downward: Vec<&TableSpec> = TABLES
        .iter()
        .filter(|spec| !device || CLIENT_PULL_SECTIONS.contains(&spec.section))
        .collect();

    // Only the client's own narrowing is reported. Upload-only is by design and is not
    // news; a section this build cannot read is.
    let mut omitted_sections = Vec::new();
    let visible: Vec<&TableSpec> = downward
        .into_iter()
        .filter(|spec| {
            let speaks = contract.speaks(spec.section);
            if !speaks {
                omitted_sections.push(spec.section.to_string());
            }
            speaks
        })
        .collect();

    // One sequence window for the whole page, so the cursor is a watermark every table
    // shares. Bounding each table by its own row count instead would let a table late in
    // foreign-key order sit below an earlier table's highest seq, and the cursor would
    // advance past it unread.
    let bound = page_bound(&state.db, &visible, since, limit).await?;
    let frontier = frontier(&state.db, &visible).await?;

    let mut sections: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for spec in visible {
        let columns = spec.columns();
        let names: Vec<String> = columns
            .iter()
            .map(|c| c.name.to_string())
            .chain(std::iter::once("server_seq".to_string()))
            .collect();

        // Only descriptor-sourced column names are interpolated.
        let sql = format!(
            "SELECT {cols} FROM {table} \
             WHERE server_seq > $1 AND server_seq <= $2 ORDER BY server_seq ASC",
            cols = names.join(", "),
            table = spec.table,
        );
        let statement = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            [since.into(), bound.into()],
        );

        let rows = state.db.query_all_raw(statement).await?;
        if rows.is_empty() {
            continue;
        }

        let mut out = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut object = serde_json::Map::with_capacity(columns.len());
            for column in &columns {
                object.insert(column.name.to_string(), column_to_json(row, column)?);
            }
            out.push(serde_json::Value::Object(object));
        }
        sections.insert(spec.section.to_string(), out);
    }

    Ok(Json(PullResponse {
        contract_version: contract.agreed(),
        cursor: bound,
        has_more: frontier > bound,
        sections,
        omitted_sections,
    }))
}

/// The `limit`-th `server_seq` above `since`, or the frontier when fewer remain.
///
/// Every table is then read to this bound, so no row can fall between two pages.
async fn page_bound<C: ConnectionTrait>(
    db: &C,
    visible: &[&TableSpec],
    since: i64,
    limit: u64,
) -> AppResult<i64> {
    if visible.is_empty() {
        return Ok(since);
    }
    // Only descriptor-sourced table names are interpolated.
    let arms: Vec<String> = visible
        .iter()
        .map(|spec| {
            format!(
                "SELECT server_seq FROM {} WHERE server_seq > $1",
                spec.table
            )
        })
        .collect();
    let sql = format!(
        "SELECT COALESCE(MAX(server_seq), $1) AS bound FROM ( \
           SELECT server_seq FROM ({}) s ORDER BY server_seq ASC LIMIT $2 \
         ) page",
        arms.join(" UNION ALL "),
    );

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            [
                since.into(),
                i64::try_from(limit).unwrap_or(i64::MAX).into(),
            ],
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "bound")?,
        None => since,
    })
}

/// The highest `server_seq` this caller can reach.
///
/// Taken over rows rather than the sequence, because `ON CONFLICT` burns sequence values
/// on rows it declines to write, which would leave `has_more` true for good.
async fn frontier<C: ConnectionTrait>(db: &C, visible: &[&TableSpec]) -> AppResult<i64> {
    if visible.is_empty() {
        return Ok(0);
    }
    // Only descriptor-sourced table names are interpolated.
    let maxima: Vec<String> = visible
        .iter()
        .map(|spec| format!("COALESCE((SELECT MAX(server_seq) FROM {}), 0)", spec.table))
        .collect();
    let sql = format!("SELECT GREATEST({}) AS frontier", maxima.join(", "));

    let row = db
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            sql,
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "frontier")?,
        None => 0,
    })
}

/// Read one column back out as JSON, in the shape a push accepts.
fn column_to_json(row: &sea_orm::QueryResult, spec: &ColumnSpec) -> AppResult<serde_json::Value> {
    fn get<T: TryGetable>(row: &sea_orm::QueryResult, name: &str) -> AppResult<Option<T>> {
        Ok(row.try_get::<Option<T>>("", name)?)
    }

    let name = spec.name;
    Ok(match spec.kind {
        ColumnKind::Uuid => get::<uuid::Uuid>(row, name)?.map_or(serde_json::Value::Null, |v| {
            serde_json::Value::String(v.to_string())
        }),
        ColumnKind::Text => get::<String>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::Float => get::<f64>(row, name)?
            .and_then(serde_json::Number::from_f64)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        ColumnKind::Int => get::<i32>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::BigInt => get::<i64>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        ColumnKind::Bool => get::<bool>(row, name)?.map_or(serde_json::Value::Null, Into::into),
        // Canonical `Z`, so the wire form is the contract's and not chrono's
        // default. `AutoSi` keeps the sub-second precision last-write-wins needs.
        ColumnKind::Timestamp => get::<chrono::DateTime<chrono::Utc>>(row, name)?
            .map_or(serde_json::Value::Null, |v| {
                serde_json::Value::String(v.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
            }),
        ColumnKind::Date => get::<chrono::NaiveDate>(row, name)?
            .map_or(serde_json::Value::Null, |v| {
                serde_json::Value::String(v.to_string())
            }),
        ColumnKind::Json => get::<serde_json::Value>(row, name)?.unwrap_or(serde_json::Value::Null),
    })
}
