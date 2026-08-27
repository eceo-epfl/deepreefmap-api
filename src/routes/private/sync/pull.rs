//! Handing a client everything it has not seen.
//!
//! The cursor is a `server_seq` high-water mark over one sequence shared by every
//! replicated table and the ledger. Deletes travel as rows with `deleted_at` set.
//!
//! A device pulls the catalogue whole ([`CLIENT_PULL_SECTIONS`]) and, from contract 2,
//! its own rows of [`OWN_ROWS_SECTIONS`] plus the ledger's decisions on what it pushed.
//! An operator gets the lot. `Deepreefmap-Sections` narrows further, and what it drops
//! comes back in `omitted_sections`.

use axum::{
    Json,
    extract::{Query, State},
};
use sea_orm::{ConnectionTrait, Statement, Value};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};

use super::ledger::Status;
use super::rows::column_to_json;
use super::schema::{OWN_ROWS_SECTIONS, OWN_ROWS_SINCE, TABLES, TableSpec, device_pulls};
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

/// The ledger's word on one entry a device pushed, decided since its cursor.
#[derive(Debug, serde::Serialize, ToSchema)]
pub struct OutboxEntry {
    pub table_key: String,
    pub row_id: uuid::Uuid,
    pub seq: i64,
    pub status: Status,
    pub reason: Option<String>,
    /// The row's `server_seq` once applied, for the device's `base_seq`.
    pub projected_seq: Option<i64>,
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
    /// Decisions on this device's entries since the cursor. Empty for an operator, and
    /// under contract 1.
    pub outbox: Vec<OutboxEntry>,
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
    let agreed = contract.agreed();
    let device = match auth {
        AuthContext::Device { device_id, .. } => Some(device_id),
        AuthContext::Keycloak { .. } => None,
    };

    let downward: Vec<&TableSpec> = TABLES
        .iter()
        .filter(|spec| device.is_none() || device_pulls(spec.section, agreed))
        .collect();

    // Only the client's own narrowing is reported.
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

    let scope = Scope {
        device,
        own_rows: device.is_some() && agreed >= OWN_ROWS_SINCE,
    };

    // One sequence window for the whole page, so the cursor is a watermark every table
    // shares and no row falls between two pages.
    let bound = page_bound(&state.db, &visible, &scope, since, limit).await?;
    let frontier = frontier(&state.db, &visible, &scope).await?;

    let mut sections: HashMap<String, Vec<serde_json::Value>> = HashMap::new();
    for spec in visible {
        let columns = spec.columns_at(agreed);
        let names: Vec<String> = columns
            .iter()
            .map(|c| c.name.to_string())
            .chain(std::iter::once("server_seq".to_string()))
            .collect();

        // Only descriptor-sourced column names are interpolated.
        let sql = format!(
            "SELECT {cols} FROM {table} \
             WHERE server_seq > $1 AND server_seq <= $2{own} ORDER BY server_seq ASC",
            cols = names.join(", "),
            table = spec.table,
            own = scope.predicate(spec),
        );
        let statement = Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            scope.bind_rows(spec, &[since.into(), bound.into()]),
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

    let outbox = if scope.own_rows {
        decided_entries(&state.db, &scope, since, bound).await?
    } else {
        Vec::new()
    };

    Ok(Json(PullResponse {
        contract_version: agreed,
        cursor: bound,
        has_more: frontier > bound,
        sections,
        omitted_sections,
        outbox,
    }))
}

/// What a device's pull is restricted to.
struct Scope {
    device: Option<uuid::Uuid>,
    /// Whether the own-rows sections and the ledger's decisions are in play.
    own_rows: bool,
}

impl Scope {
    /// `AND device_id = $3` for a section the device reads only its own rows of.
    fn predicate(&self, spec: &TableSpec) -> &'static str {
        if self.own_rows && OWN_ROWS_SECTIONS.contains(&spec.section) {
            " AND device_id = $3"
        } else {
            ""
        }
    }

    /// The binds for one section's rows: the device only where the predicate names
    /// it. A spare bind is not harmless: the pool shares prepared statements by SQL
    /// text, and one parsed with three parameters refuses a later two-bind call.
    fn bind_rows(&self, spec: &TableSpec, window: &[Value; 2]) -> Vec<Value> {
        if self.predicate(spec).is_empty() {
            window.to_vec()
        } else {
            self.bind(window)
        }
    }

    /// The two window binds, plus the device when any predicate names `$3`.
    fn bind(&self, window: &[Value; 2]) -> Vec<Value> {
        let mut values = window.to_vec();
        if self.own_rows
            && let Some(device) = self.device
        {
            values.push(device.into());
        }
        values
    }

    /// The ledger's decisions as one more arm of the window, so the cursor covers them.
    fn decided_arm(&self) -> Option<String> {
        self.own_rows.then(|| {
            "SELECT decided_seq AS server_seq FROM change_log \
             WHERE device_id = $3 AND decided_seq > $1"
                .to_string()
        })
    }
}

/// The `limit`-th `server_seq` above `since`, or the frontier when fewer remain.
async fn page_bound<C: ConnectionTrait>(
    db: &C,
    visible: &[&TableSpec],
    scope: &Scope,
    since: i64,
    limit: u64,
) -> AppResult<i64> {
    if visible.is_empty() {
        return Ok(since);
    }
    // Only descriptor-sourced table names are interpolated.
    let mut arms: Vec<String> = visible
        .iter()
        .map(|spec| {
            format!(
                "SELECT server_seq FROM {} WHERE server_seq > $1{}",
                spec.table,
                scope.predicate(spec)
            )
        })
        .collect();
    arms.extend(scope.decided_arm());
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
            scope.bind(&[
                since.into(),
                i64::try_from(limit).unwrap_or(i64::MAX).into(),
            ]),
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "bound")?,
        None => since,
    })
}

/// The highest `server_seq` this caller can reach. Over rows, not the sequence: the
/// ledger burns values no row carries.
async fn frontier<C: ConnectionTrait>(
    db: &C,
    visible: &[&TableSpec],
    scope: &Scope,
) -> AppResult<i64> {
    if visible.is_empty() {
        return Ok(0);
    }
    // Only descriptor-sourced table names are interpolated. The binds stay positional
    // so the device predicate keeps its `$3`.
    let mut maxima: Vec<String> = visible
        .iter()
        .map(|spec| {
            format!(
                "COALESCE((SELECT MAX(server_seq) FROM {} WHERE server_seq > $1{}), 0)",
                spec.table,
                scope.predicate(spec)
            )
        })
        .collect();
    if scope.own_rows {
        maxima.push(
            "COALESCE((SELECT MAX(decided_seq) FROM change_log WHERE device_id = $3 AND decided_seq > $1), 0)"
                .to_string(),
        );
    }
    let sql = format!(
        "SELECT GREATEST({}, $2::BIGINT) AS frontier",
        maxima.join(", ")
    );

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            scope.bind(&[0_i64.into(), 0_i64.into()]),
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "frontier")?,
        None => 0,
    })
}

/// Decisions on the device's entries inside the window.
async fn decided_entries<C: ConnectionTrait>(
    db: &C,
    scope: &Scope,
    since: i64,
    bound: i64,
) -> AppResult<Vec<OutboxEntry>> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT table_key, row_id, seq, status, reason, projected_seq FROM change_log \
             WHERE device_id = $3 AND decided_seq > $1 AND decided_seq <= $2 \
             ORDER BY decided_seq ASC",
            scope.bind(&[since.into(), bound.into()]),
        ))
        .await?;

    let mut out = Vec::with_capacity(rows.len());
    for row in rows {
        let status: String = row.try_get("", "status")?;
        let status = match status.as_str() {
            "applied" => Status::Applied,
            "superseded" => Status::Superseded,
            "proposed" => Status::Proposed,
            "rejected" => Status::Rejected,
            _ => Status::Dismissed,
        };
        out.push(OutboxEntry {
            table_key: row.try_get("", "table_key")?,
            row_id: row.try_get("", "row_id")?,
            seq: row.try_get("", "seq")?,
            status,
            reason: row.try_get("", "reason")?,
            projected_seq: row.try_get("", "projected_seq")?,
        });
    }
    Ok(out)
}
