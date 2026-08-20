//! The runs that consumed a clip, resolved through the passes it played in.
//!
//! One clip can play in several passes and a pass can be rerun, so "has this footage
//! been processed" is a join the console should not reimplement row by row.

use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::{EntityTrait, Statement};
use uuid::Uuid;

use crate::common::AppState;
use crate::error::AppResult;
use crate::routes::private::runs::model as run_model;
use crate::routes::private::runs::{Run, RunResponse};

/// Runs that consumed a video, newest first.
///
/// Follows `pass_video` to `transect_pass` to `run_record`, so every rerun of every
/// pass the clip played in is listed.
#[utoipa::path(
    get,
    path = "/videos/{id}/runs",
    params(("id" = Uuid, Path, description = "Video id")),
    responses(
        (status = 200, description = "Runs that consumed the video, newest first", body = [RunResponse]),
    ),
    tag = "runs"
)]
pub async fn runs_for_video(
    State(state): State<AppState>,
    Path(video_id): Path<Uuid>,
) -> AppResult<Json<Vec<RunResponse>>> {
    // Live rows at every hop: a tombstoned run, pass or link is history withdrawn.
    let sql = "\
        SELECT r.* FROM run_record r \
        JOIN transect_pass p ON p.id = r.pass_id AND p.deleted_at IS NULL \
        JOIN pass_video pv ON pv.pass_id = p.id AND pv.deleted_at IS NULL \
        WHERE pv.video_id = $1 AND r.deleted_at IS NULL \
        ORDER BY r.started_at DESC NULLS LAST, r.id";

    let found = run_model::Entity::find()
        .from_raw_sql(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            [video_id.into()],
        ))
        .all(&state.db)
        .await?;

    Ok(Json(
        found
            .into_iter()
            .map(|model| RunResponse::from(Run::from(model)))
            .collect(),
    ))
}
