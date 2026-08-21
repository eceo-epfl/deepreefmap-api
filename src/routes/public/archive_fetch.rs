//! Redeeming a signed archive fetch link: the one unauthenticated byte route.
//!
//! `/archive/{id}/download` (authenticated) mints these links so that a plain
//! browser navigation, which carries no bearer header, can still save a file. The
//! signature is object-bound, read-only and short-lived, and the bytes stream
//! from the object store through this process: nothing here exposes the store.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use chrono::Utc;
use sea_orm::{ColumnTrait, EntityTrait, QueryFilter};
use utoipa::IntoParams;
use uuid::Uuid;

use crate::archive::fetch_token;
use crate::common::AppState;
use crate::error::{AppError, AppResult};
use crate::routes::private::archive::{model as stored_object, run_artifact};

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct FetchParams {
    /// Unix timestamp the signature lapses at.
    pub expires: i64,
    /// HMAC over the object id and expiry, from `/archive/{id}/download`.
    pub sig: String,
}

/// Stream a verified object against a signed fetch link.
#[utoipa::path(
    get,
    path = "/archive/{object_id}/fetch",
    params(("object_id" = Uuid, Path, description = "Object to stream"), FetchParams),
    responses(
        (status = 200, description = "The object's bytes, as an attachment"),
        (status = 403, description = "The signature is wrong or has lapsed"),
        (status = 404, description = "No such object"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn fetch(
    State(state): State<AppState>,
    Path(object_id): Path<Uuid>,
    Query(params): Query<FetchParams>,
) -> AppResult<Response> {
    let store = state.archive.as_ref().ok_or_else(|| {
        AppError::Unavailable("The archive is not configured on this registry".to_string())
    })?;

    // The signature check comes before any row read, so an invalid link learns
    // nothing, not even whether the object exists.
    if !fetch_token::verify(
        store.fetch_secret(),
        object_id,
        params.expires,
        &params.sig,
        Utc::now().timestamp(),
    ) {
        return Err(AppError::Forbidden(
            "This download link is not valid or has lapsed: ask for a fresh one".to_string(),
        ));
    }

    let row = stored_object::Entity::find_by_id(object_id)
        .one(&state.db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No object {object_id}")))?;
    if row.status != stored_object::STATUS_COMPLETE {
        return Err(AppError::NotFound(format!("No object {object_id}")));
    }

    let filename = download_filename(&state, &row).await?;
    let (size, stream) = store.download(&row.s3_key).await?;

    let body =
        axum::body::Body::from_stream(tokio_util::io::ReaderStream::new(stream.into_async_read()));
    let disposition = format!("attachment; filename=\"{filename}\"");
    Response::builder()
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, size)
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition)
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        )
        .body(body)
        .map_err(|e| AppError::Internal(format!("fetch response: {e}")))
}

/// What the saved file is called: an artefact's own basename, or the content hash.
async fn download_filename(state: &AppState, row: &stored_object::Model) -> AppResult<String> {
    let named = run_artifact::Entity::find()
        .filter(run_artifact::Column::StoredObjectId.eq(row.id))
        .one(&state.db)
        .await?
        .and_then(|artifact| {
            artifact
                .relpath
                .rsplit('/')
                .next()
                .map(std::string::ToString::to_string)
        });
    let raw = named.unwrap_or_else(|| row.content_hash.clone());
    // Header-safe: the value sits inside a quoted string.
    Ok(raw
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect())
}
