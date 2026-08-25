//! The lines a campaign surveyed, resolved through its passes.

use axum::{
    Json,
    extract::{Path, State},
};
use sea_orm::{EntityTrait, Statement};
use uuid::Uuid;

use crate::common::AppState;
use crate::error::AppResult;
use crate::routes::private::transects::model as transect_model;
use crate::routes::private::transects::{Transect, TransectResponse};

/// Transects with a live pass in the campaign, by name.
#[utoipa::path(
    get,
    path = "/campaigns/{id}/transects",
    params(("id" = Uuid, Path, description = "Campaign id")),
    responses(
        (status = 200, description = "Transects the campaign surveyed", body = [TransectResponse]),
    ),
    tag = "transects"
)]
pub async fn transects_for_campaign(
    State(state): State<AppState>,
    Path(campaign_id): Path<Uuid>,
) -> AppResult<Json<Vec<TransectResponse>>> {
    let sql = "\
        SELECT t.* FROM transect t \
        WHERE t.deleted_at IS NULL AND t.id IN ( \
          SELECT p.transect_id FROM transect_pass p \
          WHERE p.campaign_id = $1 AND p.deleted_at IS NULL AND p.transect_id IS NOT NULL \
        ) \
        ORDER BY t.name, t.id";

    let found = transect_model::Entity::find()
        .from_raw_sql(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            [campaign_id.into()],
        ))
        .all(&state.db)
        .await?;

    Ok(Json(
        found
            .into_iter()
            .map(|model| TransectResponse::from(Transect::from(model)))
            .collect(),
    ))
}
