//! What a run archived, grouped the way the outputs tab reads it.
//!
//! A run holds three files per frame, so its artefact rows run into the thousands and
//! a generic list page cannot carry them. The counts a reader needs are aggregates, so
//! they are computed here, over the same [`crate::archive::purpose`] rule a bundle
//! download packs by.

use axum::Json;
use axum::extract::{Path, Query, State};
use sea_orm::{ConnectionTrait, Statement, Value};
use std::collections::HashMap;
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::archive::purpose;
use crate::common::AppState;
use crate::error::{AppError, AppResult};

/// Files listed in one request, and the ceiling on what a caller may ask for.
const DEFAULT_LIMIT: u64 = 200;
const MAX_LIMIT: u64 = 1000;

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct OutputGroup {
    /// A purpose, or the directory the files sit in.
    pub name: String,
    pub files: i64,
    /// Total size of the group, whatever each object's status.
    pub size_bytes: i64,
    /// How many of the group's files link an object in each status.
    pub complete: i64,
    pub failed: i64,
    pub pending: i64,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RunOutputs {
    pub run_id: Uuid,
    pub files: i64,
    pub size_bytes: i64,
    pub complete: i64,
    pub failed: i64,
    pub pending: i64,
    /// Purposes first, then directories by name.
    pub groups: Vec<OutputGroup>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct OutputFile {
    pub id: Uuid,
    pub relpath: String,
    pub size_bytes: Option<i64>,
    pub stored_object_id: Option<Uuid>,
    /// The linked object's status, absent where an artefact links none.
    pub status: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct OutputFiles {
    pub files: Vec<OutputFile>,
}

// The first path segment keeps its slash, so `purpose_of` reads the key as the
// directory it came from and the rule stays in one place.
const GROUPS_SQL: &str = "\
    SELECT CASE WHEN position('/' in ra.relpath) > 0 \
                THEN split_part(ra.relpath, '/', 1) || '/' \
                ELSE ra.relpath END AS purpose_key, \
           COUNT(*)::BIGINT AS files, \
           (COUNT(*) FILTER (WHERE so.status = 'complete'))::BIGINT AS complete, \
           (COUNT(*) FILTER (WHERE so.status = 'failed'))::BIGINT AS failed, \
           (COUNT(*) FILTER (WHERE so.status = 'pending'))::BIGINT AS pending, \
           COALESCE(SUM(COALESCE(so.size_bytes, ra.size_bytes)), 0)::BIGINT AS size_bytes \
    FROM run_artifact ra \
    LEFT JOIN stored_object so ON so.id = ra.stored_object_id \
    WHERE ra.run_id = $1 \
    GROUP BY 1";

/// What one run archived, by group.
#[utoipa::path(
    get,
    path = "/runs/{run_id}/outputs",
    params(("run_id" = Uuid, Path, description = "Run whose outputs to summarise")),
    responses(
        (status = 200, description = "The run's outputs, grouped", body = RunOutputs),
    ),
    tag = "archive"
)]
pub async fn outputs(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
) -> AppResult<Json<RunOutputs>> {
    let rows = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            GROUPS_SQL,
            [Value::from(run_id)],
        ))
        .await?;

    let mut groups: HashMap<String, OutputGroup> = HashMap::new();
    let mut total = OutputGroup {
        name: String::new(),
        files: 0,
        size_bytes: 0,
        complete: 0,
        failed: 0,
        pending: 0,
    };
    for row in &rows {
        let key: String = row.try_get("", "purpose_key")?;
        let files: i64 = row.try_get("", "files")?;
        let size_bytes: i64 = row.try_get("", "size_bytes")?;
        let complete: i64 = row.try_get("", "complete")?;
        let failed: i64 = row.try_get("", "failed")?;
        let pending: i64 = row.try_get("", "pending")?;
        let name = purpose::purpose_of(&key).to_string();
        let group = groups.entry(name.clone()).or_insert(OutputGroup {
            name,
            files: 0,
            size_bytes: 0,
            complete: 0,
            failed: 0,
            pending: 0,
        });
        group.files += files;
        group.size_bytes += size_bytes;
        group.complete += complete;
        group.failed += failed;
        group.pending += pending;
        total.files += files;
        total.size_bytes += size_bytes;
        total.complete += complete;
        total.failed += failed;
        total.pending += pending;
    }
    let mut groups: Vec<OutputGroup> = groups.into_values().collect();
    groups.sort_by(|a, b| purpose::compare(&a.name, &b.name));

    Ok(Json(RunOutputs {
        run_id,
        files: total.files,
        size_bytes: total.size_bytes,
        complete: total.complete,
        failed: total.failed,
        pending: total.pending,
        groups,
    }))
}

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct FilesParams {
    /// The group to list: a purpose, or a directory name.
    #[param(example = "frames")]
    pub purpose: String,
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

const DIRECTORY_FILES_SQL: &str = "\
    SELECT ra.id, ra.relpath, COALESCE(so.size_bytes, ra.size_bytes) AS size_bytes, \
           ra.stored_object_id, so.status \
    FROM run_artifact ra \
    LEFT JOIN stored_object so ON so.id = ra.stored_object_id \
    WHERE ra.run_id = $1 AND position('/' in ra.relpath) > 0 \
      AND split_part(ra.relpath, '/', 1) = $2 \
    ORDER BY ra.relpath \
    LIMIT $3 OFFSET $4";

const ROOT_FILES_SQL: &str = "\
    SELECT ra.id, ra.relpath, COALESCE(so.size_bytes, ra.size_bytes) AS size_bytes, \
           ra.stored_object_id, so.status \
    FROM run_artifact ra \
    LEFT JOIN stored_object so ON so.id = ra.stored_object_id \
    WHERE ra.run_id = $1 AND position('/' in ra.relpath) = 0 \
    ORDER BY ra.relpath";

fn file_of(row: &sea_orm::QueryResult) -> AppResult<OutputFile> {
    Ok(OutputFile {
        id: row.try_get("", "id")?,
        relpath: row.try_get("", "relpath")?,
        size_bytes: row.try_get("", "size_bytes")?,
        stored_object_id: row.try_get("", "stored_object_id")?,
        status: row.try_get("", "status")?,
    })
}

/// The files of one group, in path order.
#[utoipa::path(
    get,
    path = "/runs/{run_id}/outputs/files",
    params(("run_id" = Uuid, Path, description = "Run whose outputs to list"), FilesParams),
    responses(
        (status = 200, description = "The group's files", body = OutputFiles),
        (status = 400, description = "No group named"),
    ),
    tag = "archive"
)]
pub async fn files(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Query(params): Query<FilesParams>,
) -> AppResult<Json<OutputFiles>> {
    if params.purpose.is_empty() {
        return Err(AppError::BadRequest(
            "Name the group to list: a purpose, or a directory".to_string(),
        ));
    }
    let offset = params.offset.unwrap_or(0);
    let limit = params.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);

    // A root purpose is one of the rule's own buckets, not a directory, so those rows
    // are read whole and filtered here. There are tens of them.
    let root = matches!(
        params.purpose.as_str(),
        purpose::RESULTS | purpose::RECORD | purpose::WORKING
    );

    let files = if root {
        let rows = state
            .db
            .query_all_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                ROOT_FILES_SQL,
                [Value::from(run_id)],
            ))
            .await?;
        let mut kept = Vec::new();
        for row in &rows {
            let file = file_of(row)?;
            if purpose::selects(&params.purpose, &file.relpath) {
                kept.push(file);
            }
        }
        kept.into_iter()
            .skip(usize::try_from(offset).unwrap_or(usize::MAX))
            .take(usize::try_from(limit).unwrap_or(usize::MAX))
            .collect()
    } else {
        let rows = state
            .db
            .query_all_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                DIRECTORY_FILES_SQL,
                [
                    Value::from(run_id),
                    Value::from(params.purpose.clone()),
                    Value::from(i64::try_from(limit).unwrap_or(i64::MAX)),
                    Value::from(i64::try_from(offset).unwrap_or(i64::MAX)),
                ],
            ))
            .await?;
        let mut kept = Vec::with_capacity(rows.len());
        for row in &rows {
            kept.push(file_of(row)?);
        }
        kept
    };

    Ok(Json(OutputFiles { files }))
}
