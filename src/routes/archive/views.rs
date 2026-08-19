//! Upload negotiation for the blob archive.
//!
//! Devices and people both upload, but neither is trusted: no route here deletes or
//! overwrites, presigned URLs are the only write path into the bucket, and a claimed
//! hash is verified server-side before the object counts as stored.

use axum::{
    Json,
    extract::{Path, State},
};
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set, sea_query::Expr,
};
use std::sync::Arc;
use utoipa::ToSchema;
use uuid::Uuid;

use super::{model as stored_object, run_artifact};
use crate::archive::keys::{artifact_key, is_content_hash, video_key};
use crate::archive::store::{ArchiveStore, PART_SIZE_BYTES, PRESIGN_TTL_SECONDS};
use crate::common::AppState;
use crate::common::auth::{AuthContext, Origin};
use crate::error::{AppError, AppResult};
use crate::routes::runs::model as run_record;

/// The configured store, or the refusal every `/archive` route gives without one.
fn archive(state: &AppState) -> AppResult<&Arc<ArchiveStore>> {
    state.archive.as_ref().ok_or_else(|| {
        AppError::Unavailable(
            "The archive is not configured on this registry: set S3_URL, S3_BUCKET_ID, \
             S3_ACCESS_KEY, S3_SECRET_KEY and S3_PREFIX"
                .to_string(),
        )
    })
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct InitiateRequest {
    /// imohash of the file, 32 lowercase hex characters. A device already holds this
    /// for every clip it has ingested, which is what a blob and a `video_asset` meet on.
    pub content_hash: String,
    pub size_bytes: i64,
    /// `video` or `artifact`.
    pub kind: String,
    /// Required for kind `artifact`.
    #[serde(default)]
    pub run_id: Option<Uuid>,
    /// Required for kind `artifact`. Path inside the run directory.
    #[serde(default)]
    pub relpath: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PartUrl {
    pub part_number: i32,
    /// Presigned `UploadPart` URL, good for `presign_ttl_seconds`.
    pub url: String,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct InitiateResponse {
    pub object_id: Uuid,
    /// `pending` with URLs to upload, or `complete` when the content is already
    /// archived and nothing need be sent.
    pub status: String,
    pub upload_id: Option<String>,
    pub part_size_bytes: Option<i64>,
    /// How long the part URLs stay valid. A client uploading for longer than this
    /// re-initiates to mint fresh ones, so the lifetime is told, never guessed.
    pub presign_ttl_seconds: u64,
    /// Part numbers already stored, which a resuming client skips.
    pub parts_done: Vec<i32>,
    pub part_urls: Vec<PartUrl>,
}

/// Begin or resume an upload, deduplicated by content hash.
///
/// Content already archived answers `complete` with no upload at all. An unfinished
/// upload of the same content resumes wherever it stopped, whoever started it.
#[utoipa::path(
    post,
    path = "/archive/initiate",
    request_body = InitiateRequest,
    responses(
        (status = 200, description = "Upload state and any URLs still needed", body = InitiateResponse),
        (status = 400, description = "Malformed hash, size, kind or relpath"),
        (status = 404, description = "No such run"),
        (status = 409, description = "Conflicting concurrent initiate"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn initiate(
    State(state): State<AppState>,
    axum::Extension(auth): axum::Extension<AuthContext>,
    Json(body): Json<InitiateRequest>,
) -> AppResult<Json<InitiateResponse>> {
    let store = archive(&state)?.clone();
    if !is_content_hash(&body.content_hash) {
        return Err(AppError::BadRequest(
            "content_hash must be 32 lowercase hex characters".to_string(),
        ));
    }
    let max = state.config.archive_max_object_bytes;
    if body.size_bytes < 1 || body.size_bytes > max {
        return Err(AppError::BadRequest(format!(
            "size_bytes must be between 1 and {max}"
        )));
    }

    let (key, linkage) = resolve_target(&state, &store, &body).await?;
    let response = negotiate(&state, &store, &auth, &body, key).await?;

    if let Some((run_id, relpath)) = linkage {
        upsert_run_artifact(&state.db, run_id, &relpath, &body, response.object_id).await?;
    }

    Ok(Json(response))
}

/// The key an upload of this content would go to, and the artefact linkage the request
/// asks for. Both validated before any row or S3 state is touched.
async fn resolve_target(
    state: &AppState,
    store: &ArchiveStore,
    body: &InitiateRequest,
) -> AppResult<(String, Option<(Uuid, String)>)> {
    match body.kind.as_str() {
        stored_object::KIND_VIDEO => Ok((video_key(&store.prefix, &body.content_hash), None)),
        stored_object::KIND_ARTIFACT => {
            let (Some(run_id), Some(relpath)) = (body.run_id, body.relpath.as_deref()) else {
                return Err(AppError::BadRequest(
                    "kind artifact requires run_id and relpath".to_string(),
                ));
            };
            let key = artifact_key(&store.prefix, run_id, relpath).ok_or_else(|| {
                AppError::BadRequest(
                    "relpath must be a plain relative path inside the run directory".to_string(),
                )
            })?;
            let run_exists = run_record::Entity::find_by_id(run_id)
                .one(&state.db)
                .await?
                .is_some();
            if !run_exists {
                return Err(AppError::NotFound(format!("No run {run_id}")));
            }
            Ok((key, Some((run_id, relpath.to_string()))))
        }
        other => Err(AppError::BadRequest(format!(
            "Unknown kind {other}: expected video or artifact"
        ))),
    }
}

/// Answer for the content's current state: dedup, resume, restart or a new upload.
async fn negotiate(
    state: &AppState,
    store: &Arc<ArchiveStore>,
    auth: &AuthContext,
    body: &InitiateRequest,
    key: String,
) -> AppResult<InitiateResponse> {
    let existing = stored_object::Entity::find()
        .filter(stored_object::Column::ContentHash.eq(&body.content_hash))
        .one(&state.db)
        .await?;

    match existing {
        // The dedup answer: the content is archived, nothing travels.
        Some(row) if row.status == stored_object::STATUS_COMPLETE => {
            Ok(answer(row.id, stored_object::STATUS_COMPLETE))
        }
        Some(row) if row.status == stored_object::STATUS_PENDING => {
            if row.size_bytes != body.size_bytes {
                return Err(AppError::Conflict(format!(
                    "This content was initiated with size {}, not {}",
                    row.size_bytes, body.size_bytes
                )));
            }
            resume(state, store, row).await
        }
        // A failed upload starts over on the same row.
        Some(row) => {
            if let Some(old_upload) = row.s3_upload_id.as_deref() {
                store.abort_multipart(&row.s3_key, old_upload).await;
            }
            let (key, size) = (row.s3_key.clone(), body.size_bytes);
            let mut restart: stored_object::ActiveModel = row.into();
            restart.size_bytes = Set(size);
            attribute(&mut restart, auth);
            let restarted = restart.update(&state.db).await?;
            fresh_upload(state, store, restarted, &key).await
        }
        None => {
            let mut new_row = stored_object::ActiveModel {
                id: Set(Uuid::new_v4()),
                content_hash: Set(body.content_hash.clone()),
                size_bytes: Set(body.size_bytes),
                kind: Set(body.kind.clone()),
                status: Set(stored_object::STATUS_PENDING.to_string()),
                s3_key: Set(key.clone()),
                s3_upload_id: Set(None),
                part_size_bytes: Set(Some(PART_SIZE_BYTES)),
                created_at: Set(Utc::now()),
                updated_at: Set(Utc::now()),
                last_part_at: Set(None),
                completed_at: Set(None),
                failure: Set(None),
                ..Default::default()
            };
            attribute(&mut new_row, auth);
            let inserted = match new_row.insert(&state.db).await {
                Ok(row) => row,
                // A racing initiate of the same content won the unique index.
                Err(e)
                    if matches!(
                        e.sql_err(),
                        Some(sea_orm::SqlErr::UniqueConstraintViolation(_))
                    ) =>
                {
                    return Err(AppError::Conflict(
                        "This content is being initiated concurrently: retry".to_string(),
                    ));
                }
                Err(e) => return Err(e.into()),
            };
            fresh_upload(state, store, inserted, &key).await
        }
    }
}

/// Who is sending the bytes, for the audit columns.
fn attribute(row: &mut stored_object::ActiveModel, auth: &AuthContext) {
    match auth.origin() {
        Origin::Human { sub } => {
            row.uploaded_by = Set(Some(sub.to_string()));
            row.uploaded_by_device_id = Set(None);
        }
        Origin::Device { device_id, .. } => {
            row.uploaded_by = Set(None);
            row.uploaded_by_device_id = Set(Some(device_id));
        }
    }
}

fn answer(object_id: Uuid, status: &str) -> InitiateResponse {
    InitiateResponse {
        object_id,
        status: status.to_string(),
        upload_id: None,
        part_size_bytes: None,
        presign_ttl_seconds: PRESIGN_TTL_SECONDS,
        parts_done: Vec::new(),
        part_urls: Vec::new(),
    }
}

/// Resume a pending upload: report the parts S3 already holds and presign the rest.
///
/// An upload S3 no longer knows starts over, losing progress rather than wedging.
async fn resume(
    state: &AppState,
    store: &ArchiveStore,
    row: stored_object::Model,
) -> AppResult<InitiateResponse> {
    let done = match row.s3_upload_id.as_deref() {
        Some(upload_id) => store
            .uploaded_part_numbers(&row.s3_key, upload_id)
            .await
            .ok(),
        None => None,
    };
    let Some(parts_done) = done else {
        let key = row.s3_key.clone();
        return fresh_upload(state, store, row, &key).await;
    };
    let upload_id = row.s3_upload_id.clone().expect("checked above");
    let part_size = row.part_size_bytes.unwrap_or(PART_SIZE_BYTES);

    // Resume counts as part activity, so the reaper's idle clock restarts.
    let mut touch: stored_object::ActiveModel = row.clone().into();
    touch.last_part_at = Set(Some(Utc::now()));
    touch.updated_at = Set(Utc::now());
    touch.update(&state.db).await?;

    let part_urls = presign_missing(
        store,
        &row.s3_key,
        &upload_id,
        row.size_bytes,
        part_size,
        &parts_done,
    )
    .await?;
    Ok(InitiateResponse {
        object_id: row.id,
        status: stored_object::STATUS_PENDING.to_string(),
        upload_id: Some(upload_id),
        part_size_bytes: Some(part_size),
        presign_ttl_seconds: PRESIGN_TTL_SECONDS,
        parts_done,
        part_urls,
    })
}

/// Open a new multipart upload for a pending row and presign every part.
async fn fresh_upload(
    state: &AppState,
    store: &ArchiveStore,
    row: stored_object::Model,
    key: &str,
) -> AppResult<InitiateResponse> {
    let upload_id = store.create_multipart(key).await?;

    let (object_id, size_bytes) = (row.id, row.size_bytes);
    let mut update: stored_object::ActiveModel = row.into();
    update.status = Set(stored_object::STATUS_PENDING.to_string());
    update.s3_upload_id = Set(Some(upload_id.clone()));
    update.part_size_bytes = Set(Some(PART_SIZE_BYTES));
    update.last_part_at = Set(Some(Utc::now()));
    update.updated_at = Set(Utc::now());
    update.completed_at = Set(None);
    update.failure = Set(None);
    update.update(&state.db).await?;

    let part_urls =
        presign_missing(store, key, &upload_id, size_bytes, PART_SIZE_BYTES, &[]).await?;
    Ok(InitiateResponse {
        object_id,
        status: stored_object::STATUS_PENDING.to_string(),
        upload_id: Some(upload_id),
        part_size_bytes: Some(PART_SIZE_BYTES),
        presign_ttl_seconds: PRESIGN_TTL_SECONDS,
        parts_done: Vec::new(),
        part_urls,
    })
}

async fn presign_missing(
    store: &ArchiveStore,
    key: &str,
    upload_id: &str,
    size_bytes: i64,
    part_size: i64,
    parts_done: &[i32],
) -> AppResult<Vec<PartUrl>> {
    let count = i32::try_from((size_bytes + part_size - 1) / part_size)
        .map_err(|_| AppError::BadRequest("size_bytes needs too many parts".to_string()))?;
    let mut urls = Vec::new();
    for part_number in 1..=count {
        if parts_done.contains(&part_number) {
            continue;
        }
        urls.push(PartUrl {
            part_number,
            url: store
                .presign_upload_part(key, upload_id, part_number)
                .await?,
        });
    }
    Ok(urls)
}

/// Record which blob a run directory path holds.
///
/// One row per `(run_id, relpath)`. An existing row keeps its link while the object it
/// names is complete, so a later upload cannot silently replace a verified artefact.
async fn upsert_run_artifact<C: ConnectionTrait>(
    db: &C,
    run_id: Uuid,
    relpath: &str,
    body: &InitiateRequest,
    object_id: Uuid,
) -> AppResult<()> {
    let existing = run_artifact::Entity::find()
        .filter(run_artifact::Column::RunId.eq(run_id))
        .filter(run_artifact::Column::Relpath.eq(relpath))
        .one(db)
        .await?;

    let Some(row) = existing else {
        run_artifact::ActiveModel {
            id: Set(Uuid::new_v4()),
            run_id: Set(run_id),
            relpath: Set(relpath.to_string()),
            kind: Set(None),
            size_bytes: Set(Some(body.size_bytes)),
            content_hash: Set(body.content_hash.clone()),
            stored_object_id: Set(Some(object_id)),
            created_at: Set(Utc::now()),
            updated_at: Set(Utc::now()),
        }
        .insert(db)
        .await?;
        return Ok(());
    };

    if let Some(linked) = row.stored_object_id
        && linked != object_id
    {
        let linked_complete = stored_object::Entity::find_by_id(linked)
            .filter(stored_object::Column::Status.eq(stored_object::STATUS_COMPLETE))
            .one(db)
            .await?
            .is_some();
        if linked_complete {
            return Ok(());
        }
    }

    let mut update: run_artifact::ActiveModel = row.into();
    update.content_hash = Set(body.content_hash.clone());
    update.size_bytes = Set(Some(body.size_bytes));
    update.stored_object_id = Set(Some(object_id));
    update.updated_at = Set(Utc::now());
    update.update(db).await?;
    Ok(())
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct CompletedPartBody {
    pub part_number: i32,
    pub etag: String,
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct CompleteRequest {
    /// Accepted for compatibility and ignored: the server assembles from its
    /// own `ListParts`, because a resuming client cannot know the `ETag`s of
    /// parts an earlier attempt sent.
    #[serde(default)]
    pub parts: Vec<CompletedPartBody>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct CompleteResponse {
    pub object_id: Uuid,
    /// `complete`: S3 assembled the parts into the finished object.
    pub status: String,
}

/// Assemble the uploaded parts into the finished object.
///
/// S3 checked every part against the `ETag` it answered as the part arrived, so an
/// assembly it accepts is the bytes the client sent.
#[utoipa::path(
    post,
    path = "/archive/{object_id}/complete",
    params(("object_id" = Uuid, Path, description = "Object being uploaded")),
    request_body = CompleteRequest,
    responses(
        (status = 200, description = "Parts assembled into the object", body = CompleteResponse),
        (status = 404, description = "No such object"),
        (status = 409, description = "The object is not pending"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn complete(
    State(state): State<AppState>,
    Path(object_id): Path<Uuid>,
    Json(_body): Json<CompleteRequest>,
) -> AppResult<Json<CompleteResponse>> {
    let store = archive(&state)?.clone();
    let row = stored_object::Entity::find_by_id(object_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No object {object_id}")))?;
    if row.status != stored_object::STATUS_PENDING {
        return Err(AppError::Conflict(format!(
            "Object is {}, not pending",
            row.status
        )));
    }
    let upload_id = row
        .s3_upload_id
        .clone()
        .ok_or_else(|| AppError::Conflict("Object has no open upload".to_string()))?;

    let assembled = store.complete_multipart(&row.s3_key, &upload_id).await?;
    if assembled == 0 {
        return Err(AppError::Conflict(
            "Nothing has been uploaded for this object yet".to_string(),
        ));
    }

    // Guarded on status, so a racing complete flips the row exactly once.
    let flipped = stored_object::Entity::update_many()
        .col_expr(
            stored_object::Column::Status,
            Expr::value(stored_object::STATUS_COMPLETE),
        )
        .col_expr(
            stored_object::Column::S3UploadId,
            Expr::value(Option::<String>::None),
        )
        .col_expr(stored_object::Column::CompletedAt, Expr::value(Utc::now()))
        .col_expr(stored_object::Column::UpdatedAt, Expr::value(Utc::now()))
        .filter(stored_object::Column::Id.eq(object_id))
        .filter(stored_object::Column::Status.eq(stored_object::STATUS_PENDING))
        .exec(&state.db)
        .await?;
    if flipped.rows_affected != 1 {
        return Err(AppError::Conflict(
            "Object is no longer pending".to_string(),
        ));
    }

    Ok(Json(CompleteResponse {
        object_id,
        status: stored_object::STATUS_COMPLETE.to_string(),
    }))
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct DownloadResponse {
    /// Presigned `GetObject` URL, good for `PRESIGN_TTL_SECONDS`.
    pub url: String,
}

/// A short-lived download URL for a verified object.
#[utoipa::path(
    get,
    path = "/archive/{object_id}/download",
    params(("object_id" = Uuid, Path, description = "Object to download")),
    responses(
        (status = 200, description = "Presigned download URL", body = DownloadResponse),
        (status = 404, description = "No such object"),
        (status = 409, description = "The object is not complete yet"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn download(
    State(state): State<AppState>,
    Path(object_id): Path<Uuid>,
) -> AppResult<Json<DownloadResponse>> {
    let store = archive(&state)?;
    let row = stored_object::Entity::find_by_id(object_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No object {object_id}")))?;
    if row.status != stored_object::STATUS_COMPLETE {
        return Err(AppError::Conflict(format!(
            "Object is {}, not complete",
            row.status
        )));
    }
    Ok(Json(DownloadResponse {
        url: store.presign_get(&row.s3_key).await?,
    }))
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ByHashResponse {
    pub object_id: Uuid,
    pub status: String,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// Whether content with this hash is archived. Cheap, for badges.
#[utoipa::path(
    get,
    path = "/archive/by-hash/{content_hash}",
    params(("content_hash" = String, Path, description = "imohash, 32 lowercase hex")),
    responses(
        (status = 200, description = "The object's state", body = ByHashResponse),
        (status = 400, description = "Malformed hash"),
        (status = 404, description = "Nothing archived under this hash"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn by_hash(
    State(state): State<AppState>,
    Path(content_hash): Path<String>,
) -> AppResult<Json<ByHashResponse>> {
    archive(&state)?;
    if !is_content_hash(&content_hash) {
        return Err(AppError::BadRequest(
            "content_hash must be 32 lowercase hex characters".to_string(),
        ));
    }
    let row = stored_object::Entity::find()
        .filter(stored_object::Column::ContentHash.eq(&content_hash))
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound("Nothing archived under this hash".to_string()))?;
    Ok(Json(ByHashResponse {
        object_id: row.id,
        status: row.status,
        completed_at: row.completed_at,
    }))
}

/// Ceiling on one probe, so a badge refresh cannot become an unbounded query.
const PROBE_MAX_HASHES: usize = 500;
/// Ceiling on one runs probe, sized to a page of runs in the console.
const PROBE_MAX_RUNS: usize = 200;

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct ProbeRequest {
    /// Content hashes to look up, 32 lowercase hex each, at most 500.
    pub hashes: Vec<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ProbeState {
    pub object_id: Uuid,
    pub status: String,
    pub completed_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct ProbeResponse {
    /// One entry per hash that has a row. Hashes never seen are simply absent.
    pub states: std::collections::HashMap<String, ProbeState>,
}

/// Archive state for many hashes at once, so badges cost one request per page.
#[utoipa::path(
    post,
    path = "/archive/probe",
    request_body = ProbeRequest,
    responses(
        (status = 200, description = "State per known hash", body = ProbeResponse),
        (status = 400, description = "Malformed hash or too many of them"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn probe(
    State(state): State<AppState>,
    Json(body): Json<ProbeRequest>,
) -> AppResult<Json<ProbeResponse>> {
    archive(&state)?;
    if body.hashes.len() > PROBE_MAX_HASHES {
        return Err(AppError::BadRequest(format!(
            "At most {PROBE_MAX_HASHES} hashes per probe"
        )));
    }
    if let Some(bad) = body.hashes.iter().find(|hash| !is_content_hash(hash)) {
        return Err(AppError::BadRequest(format!(
            "{bad}: content_hash must be 32 lowercase hex characters"
        )));
    }
    let states = stored_object::Entity::find()
        .filter(stored_object::Column::ContentHash.is_in(&body.hashes))
        .all(&state.db)
        .await?
        .into_iter()
        .map(|row| {
            (
                row.content_hash,
                ProbeState {
                    object_id: row.id,
                    status: row.status,
                    completed_at: row.completed_at,
                },
            )
        })
        .collect();
    Ok(Json(ProbeResponse { states }))
}

#[derive(Debug, serde::Deserialize, ToSchema)]
pub struct RunsProbeRequest {
    /// Runs to look up, at most 200.
    pub run_ids: Vec<Uuid>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RunArchiveState {
    /// How many artefact rows the run has.
    pub artifacts: i64,
    /// How many of them link a stored object in status `complete`.
    pub complete: i64,
    /// And how many link one in status `failed`.
    pub failed: i64,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct RunsProbeResponse {
    /// One entry per run id with artefact rows. Runs without any are absent.
    pub states: std::collections::HashMap<String, RunArchiveState>,
}

/// Archive state for many runs at once: artefact counts, grouped in one query.
#[utoipa::path(
    post,
    path = "/archive/runs-probe",
    request_body = RunsProbeRequest,
    responses(
        (status = 200, description = "Counts per run with artefacts", body = RunsProbeResponse),
        (status = 400, description = "Too many run ids"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn runs_probe(
    State(state): State<AppState>,
    Json(body): Json<RunsProbeRequest>,
) -> AppResult<Json<RunsProbeResponse>> {
    archive(&state)?;
    if body.run_ids.len() > PROBE_MAX_RUNS {
        return Err(AppError::BadRequest(format!(
            "At most {PROBE_MAX_RUNS} run ids per probe"
        )));
    }
    if body.run_ids.is_empty() {
        return Ok(Json(RunsProbeResponse {
            states: std::collections::HashMap::new(),
        }));
    }

    // Only compile-time status constants and generated placeholders are interpolated.
    let placeholders: Vec<String> = (1..=body.run_ids.len()).map(|i| format!("${i}")).collect();
    let sql = format!(
        "SELECT ra.run_id, \
                COUNT(*) AS artifacts, \
                COUNT(*) FILTER (WHERE so.status = '{complete}') AS complete, \
                COUNT(*) FILTER (WHERE so.status = '{failed}') AS failed \
         FROM run_artifact ra \
         LEFT JOIN stored_object so ON so.id = ra.stored_object_id \
         WHERE ra.run_id IN ({ids}) \
         GROUP BY ra.run_id",
        complete = stored_object::STATUS_COMPLETE,
        failed = stored_object::STATUS_FAILED,
        ids = placeholders.join(", "),
    );
    let binds: Vec<sea_orm::Value> = body.run_ids.iter().map(|id| (*id).into()).collect();
    let found = state
        .db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            binds,
        ))
        .await?;

    let mut states = std::collections::HashMap::with_capacity(found.len());
    for row in &found {
        let run_id: Uuid = row.try_get("", "run_id")?;
        states.insert(
            run_id.to_string(),
            RunArchiveState {
                artifacts: row.try_get("", "artifacts")?,
                complete: row.try_get("", "complete")?,
                failed: row.try_get("", "failed")?,
            },
        );
    }
    Ok(Json(RunsProbeResponse { states }))
}
