//! Minting a link that packs one run's outputs into a single zip.
//!
//! A run arrives as hundreds of separate objects, and a reader who wants the results
//! wants them as one file. The link is signed the same way a single-object fetch link
//! is, so a plain browser navigation redeems it and the object store stays unreachable.

use axum::Json;
use axum::extract::{Path, Query, State};
use chrono::Utc;
use sea_orm::{ConnectionTrait, EntityTrait, Statement, Value};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::archive::keys::relpath_is_safe;
use crate::archive::{fetch_token, purpose};
use crate::common::AppState;
use crate::error::{AppError, AppResult};
use crate::routes::private::runs::model as run_record;

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct BundleParams {
    /// Which group to pack: a purpose, a directory name, or `all`.
    #[param(example = "Results")]
    pub purpose: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct BundleResponse {
    /// A signed link on this registry that streams the zip.
    pub url: String,
    /// Files the zip will hold.
    pub file_count: i64,
    /// Their total size, before zipping. Stored, not deflated, so this is close.
    pub total_bytes: i64,
    pub filename: String,
}

/// One archived file of a run: where it sits in the run directory and in the store.
pub struct BundleEntry {
    pub relpath: String,
    pub s3_key: String,
}

const ENTRIES_SQL: &str = "\
    SELECT ra.relpath, so.s3_key \
    FROM run_artifact ra \
    JOIN stored_object so ON so.id = ra.stored_object_id \
    WHERE ra.run_id = $1 AND so.status = 'complete' \
    ORDER BY ra.relpath";

/// The complete files of one run, in path order, filtered to the group asked for.
pub async fn entries<C: ConnectionTrait>(
    db: &C,
    run_id: Uuid,
    purpose: &str,
) -> AppResult<Vec<BundleEntry>> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            ENTRIES_SQL,
            [Value::from(run_id)],
        ))
        .await?;
    let mut out = Vec::with_capacity(rows.len());
    for row in &rows {
        let relpath: String = row.try_get("", "relpath")?;
        if !purpose::selects(purpose, &relpath) {
            continue;
        }
        out.push(BundleEntry {
            relpath,
            s3_key: row.try_get("", "s3_key")?,
        });
    }
    Ok(out)
}

/// The name of the run directory the files came from, for the zip's top folder.
///
/// A device names it, so it is refused unless it is one safe path component: it goes
/// on to every entry path in the zip, where a `..` would land outside the folder.
pub async fn run_folder<C: ConnectionTrait>(db: &C, run_id: Uuid) -> AppResult<String> {
    let run = run_record::Entity::find_by_id(run_id)
        .one(db)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("No run {run_id}")))?;
    Ok(if is_one_component(&run.run_dir_name) {
        run.run_dir_name
    } else {
        run_id.to_string()
    })
}

fn is_one_component(name: &str) -> bool {
    !name.is_empty() && !name.contains('/') && relpath_is_safe(name)
}

const SIZES_SQL: &str = "\
    SELECT ra.relpath, so.size_bytes \
    FROM run_artifact ra \
    JOIN stored_object so ON so.id = ra.stored_object_id \
    WHERE ra.run_id = $1 AND so.status = 'complete'";

/// A signed link that streams one run's outputs, or one group of them, as a zip.
#[utoipa::path(
    get,
    path = "/runs/{run_id}/outputs/bundle",
    params(("run_id" = Uuid, Path, description = "Run whose outputs to pack"), BundleParams),
    responses(
        (status = 200, description = "Signed bundle URL on this registry", body = BundleResponse),
        (status = 404, description = "No such run, or nothing archived in that group"),
        (status = 503, description = "The archive is not configured"),
    ),
    tag = "archive"
)]
pub async fn bundle(
    State(state): State<AppState>,
    Path(run_id): Path<Uuid>,
    Query(params): Query<BundleParams>,
) -> AppResult<Json<BundleResponse>> {
    let store = state.archive.as_ref().ok_or_else(|| {
        AppError::Unavailable("The archive is not configured on this registry".to_string())
    })?;
    let group = params
        .purpose
        .unwrap_or_else(|| purpose::EVERYTHING.to_string());
    let folder = run_folder(&state.db, run_id).await?;

    let rows = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            SIZES_SQL,
            [Value::from(run_id)],
        ))
        .await?;
    let mut file_count = 0_i64;
    let mut total_bytes = 0_i64;
    for row in &rows {
        let relpath: String = row.try_get("", "relpath")?;
        if !purpose::selects(&group, &relpath) {
            continue;
        }
        file_count += 1;
        total_bytes += row.try_get::<i64>("", "size_bytes").unwrap_or(0);
    }
    if file_count == 0 {
        return Err(AppError::NotFound(format!(
            "Run {run_id} has nothing archived under {group}"
        )));
    }

    let expires = Utc::now().timestamp() + fetch_token::FETCH_TTL_SECONDS;
    let sig = fetch_token::sign_bundle(store.fetch_secret(), run_id, &group, expires);
    let encoded: String = form_urlencoded::byte_serialize(group.as_bytes()).collect();
    let path =
        format!("/archive/runs/{run_id}/outputs.zip?purpose={encoded}&expires={expires}&sig={sig}");
    let url = match state.config.public_base_url.as_deref() {
        Some(base) => format!("{}{path}", base.trim_end_matches('/')),
        None => path,
    };
    Ok(Json(BundleResponse {
        url,
        file_count,
        total_bytes,
        filename: format!("{folder}-{}.zip", purpose::slug(&group)),
    }))
}
