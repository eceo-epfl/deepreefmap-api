//! Redeeming a signed bundle link: one run's outputs, zipped as they stream.
//!
//! Nothing is buffered on the way through. The objects are stored, not deflated, into
//! the archive: they are already compressed formats, and a stored entry costs one pass
//! over the bytes with no memory held per file.

use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, header};
use axum::response::Response;
use async_zip::{Compression, ZipEntryBuilder};
use chrono::Utc;
use utoipa::IntoParams;
use uuid::Uuid;

use tokio_util::compat::FuturesAsyncWriteCompatExt;

use crate::archive::{fetch_token, purpose};
use crate::common::AppState;
use crate::error::{AppError, AppResult};
use crate::routes::private::archive::bundle::{BundleEntry, entries, run_folder};

/// Enough to keep the object reads ahead of the client without holding a part.
const PIPE_BYTES: usize = 256 * 1024;

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct BundleParams {
    /// The group named when the link was signed.
    pub purpose: String,
    /// Unix timestamp the signature lapses at.
    pub expires: i64,
    /// HMAC over the run, group and expiry, from `/runs/{id}/outputs/bundle`.
    pub sig: String,
}

/// Stream one run's outputs as a zip against a signed bundle link.
#[utoipa::path(
    get,
    path = "/archive/runs/{run_id}/outputs.zip",
    params(("run_id" = Uuid, Path, description = "Run whose outputs to stream"), BundleParams),
    responses(
        (status = 200, description = "The zip, as an attachment"),
        (status = 403, description = "The signature is wrong or has lapsed"),
        (status = 404, description = "No such run, or nothing archived in that group"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn bundle(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Query(params): Query<BundleParams>,
) -> AppResult<Response> {
    let store = state.archive.as_ref().ok_or_else(|| {
        AppError::Unavailable("The archive is not configured on this registry".to_string())
    })?;

    // The signature check comes before any row read, so an invalid link learns
    // nothing, not even whether the run exists.
    if !fetch_token::verify_bundle(
        store.fetch_secret(),
        run_id,
        &params.purpose,
        params.expires,
        &params.sig,
        Utc::now().timestamp(),
    ) {
        return Err(AppError::Forbidden(
            "This download link is not valid or has lapsed: ask for a fresh one".to_string(),
        ));
    }

    let folder = run_folder(&state.db, run_id).await?;
    let files = entries(&state.db, run_id, &params.purpose).await?;
    if files.is_empty() {
        return Err(AppError::NotFound(format!(
            "Run {run_id} has nothing archived under {}",
            params.purpose
        )));
    }

    let (writer, reader) = tokio::io::duplex(PIPE_BYTES);
    let store = store.clone();
    let folder_in_zip = folder.clone();
    tokio::spawn(async move {
        if let Err(error) = pack(writer, store, files, folder_in_zip).await {
            // The client sees a truncated zip; the reason belongs in the log.
            tracing::warn!(%run_id, %error, "Bundle stream ended early");
        }
    });

    let filename = format!("{folder}-{}.zip", purpose::slug(&params.purpose));
    let disposition = format!("attachment; filename=\"{}\"", safe_name(&filename));
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(
            header::CONTENT_DISPOSITION,
            HeaderValue::from_str(&disposition)
                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
        )
        .body(axum::body::Body::from_stream(
            tokio_util::io::ReaderStream::new(reader),
        ))
        .map_err(|e| AppError::Internal(format!("bundle response: {e}")))
}

async fn pack(
    writer: tokio::io::DuplexStream,
    store: std::sync::Arc<crate::archive::store::ArchiveStore>,
    files: Vec<BundleEntry>,
    folder: String,
) -> Result<(), String> {
    let mut zip = async_zip::tokio::write::ZipFileWriter::with_tokio(writer);
    for file in files {
        let (_, object) = store
            .download(&file.s3_key)
            .await
            .map_err(|error| format!("{}: {error}", file.relpath))?;
        let builder = ZipEntryBuilder::new(
            format!("{folder}/{}", file.relpath).into(),
            Compression::Stored,
        );
        let entry = zip
            .write_entry_stream(builder)
            .await
            .map_err(|error| format!("{}: {error}", file.relpath))?;
        let mut source = object.into_async_read();
        let mut sink = entry.compat_write();
        tokio::io::copy(&mut source, &mut sink)
            .await
            .map_err(|error| format!("{}: {error}", file.relpath))?;
        sink.into_inner()
            .close()
            .await
            .map_err(|error| format!("{}: {error}", file.relpath))?;
    }
    zip.close().await.map_err(|error| error.to_string())?;
    Ok(())
}

/// Header-safe: the value sits inside a quoted string.
fn safe_name(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect()
}
