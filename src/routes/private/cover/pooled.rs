//! Cover pooled over a transect, computed once here.
//!
//! Every pass along a line measures the same reef, so the transect figure is the counts
//! summed over the summed denominator, not the mean of the per-pass fractions. Averaging
//! fractions lets a ten-second pass weigh as much as a three-minute one.
//!
//! One implementation, because two disagree: the desktop application and the console each
//! had their own, and they reported different numbers from identical rows.

use axum::{
    Json,
    extract::{Path, Query, State},
};
use sea_orm::{ConnectionTrait, Statement};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::common::AppState;
use crate::error::{AppError, AppResult};

#[derive(Debug, serde::Deserialize, IntoParams)]
pub struct PooledParams {
    /// Class hierarchy level: `fine`, `intermediate` or `coarse`.
    #[serde(default = "default_level")]
    pub level: String,
    /// Narrow to the passes swum during one expedition. Omit for the whole transect.
    #[serde(default)]
    pub campaign_id: Option<Uuid>,
}

fn default_level() -> String {
    "coarse".to_string()
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct GroupCover {
    pub class_group: String,
    /// Share of the pooled denominator, 0 to 1.
    pub fraction: f64,
    pub point_count: f64,
    /// `#rrggbb`, from the published class group table.
    pub colour: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PooledCover {
    pub transect_id: Uuid,
    pub campaign_id: Option<Uuid>,
    pub level: String,
    /// Points the fractions are measured over.
    pub denominator: f64,
    /// Passes that contributed a run with counts.
    pub contributing_passes: i64,
    /// Passes on this transect, so a partial figure is visible as partial.
    pub expected_passes: i64,
    /// Largest fraction first.
    pub groups: Vec<GroupCover>,
}

/// Cover pooled across a transect's passes.
///
/// Collapses reruns to the latest succeeded run per pass, so a pass processed twice counts
/// once.
#[utoipa::path(
    get,
    path = "/transects/{id}/cover",
    params(("id" = Uuid, Path, description = "Transect id"), PooledParams),
    responses(
        (status = 200, description = "Pooled cover for the transect", body = PooledCover),
        (status = 400, description = "Unknown level"),
    ),
    tag = "cover"
)]
pub async fn pooled_cover(
    State(state): State<AppState>,
    Path(transect_id): Path<Uuid>,
    Query(params): Query<PooledParams>,
) -> AppResult<Json<PooledCover>> {
    let level = params.level;
    if !crate::contract::vocab::COVER_LEVEL
        .codes()
        .contains(&level.as_str())
    {
        return Err(AppError::BadRequest(format!(
            "Unknown level {level}: expected fine, intermediate or coarse"
        )));
    }

    // A null campaign is its own bucket, not a match for every campaign.
    let campaign_filter = match params.campaign_id {
        Some(_) => "AND p.campaign_id = $3",
        None => "",
    };
    let expected_passes = count_passes(&state.db, transect_id, params.campaign_id).await?;

    // `latest` picks one run per pass. `started_at DESC, id DESC` is deterministic: a
    // whole survey arrives in one push, so `created_at` ties across every row of it.
    let sql = format!(
        "\
        WITH latest AS ( \
          SELECT DISTINCT ON (r.pass_id) r.id \
          FROM run_record r \
          JOIN transect_pass p ON p.id = r.pass_id \
          WHERE p.transect_id = $1 AND p.deleted_at IS NULL {campaign_filter} \
            AND r.deleted_at IS NULL AND r.status = 'succeeded' \
          ORDER BY r.pass_id, r.started_at DESC NULLS LAST, r.id DESC \
        ), \
        rows AS ( \
          SELECT c.class_group, c.point_count, c.denominator, c.run_id \
          FROM cover_row c JOIN latest ON latest.id = c.run_id \
          WHERE c.deleted_at IS NULL AND c.level = $2 AND c.estimator = 'per_pass' \
            AND c.point_count IS NOT NULL AND c.denominator IS NOT NULL \
        ), \
        totals AS ( \
          SELECT SUM(denominator) AS denominator, COUNT(*)::BIGINT AS passes \
          FROM (SELECT DISTINCT run_id, denominator FROM rows) d \
        ) \
        SELECT rows.class_group, \
               SUM(rows.point_count) AS point_count, \
               (SELECT denominator FROM totals) AS denominator, \
               (SELECT passes FROM totals) AS contributing_passes \
        FROM rows GROUP BY rows.class_group ORDER BY point_count DESC, rows.class_group ASC"
    );

    let mut binds: Vec<sea_orm::Value> = vec![transect_id.into(), level.clone().into()];
    if let Some(campaign_id) = params.campaign_id {
        binds.push(campaign_id.into());
    }
    let found = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            binds,
        ))
        .await?;

    let mut denominator = 0.0_f64;
    let mut contributing_passes = 0_i64;
    let mut groups = Vec::with_capacity(found.len());
    for row in &found {
        denominator = row.try_get("", "denominator").unwrap_or(0.0);
        contributing_passes = row.try_get("", "contributing_passes").unwrap_or(0);
        let class_group: String = row.try_get("", "class_group")?;
        let point_count: f64 = row.try_get("", "point_count").unwrap_or(0.0);
        let colour = colour_of(&level, &class_group);
        groups.push(GroupCover {
            class_group,
            fraction: if denominator > 0.0 {
                point_count / denominator
            } else {
                0.0
            },
            point_count,
            colour,
        });
    }

    Ok(Json(PooledCover {
        transect_id,
        campaign_id: params.campaign_id,
        level,
        denominator,
        contributing_passes,
        expected_passes,
        groups,
    }))
}

pub(super) fn colour_of(level: &str, group: &str) -> Option<String> {
    crate::contract::classes::CLASS_GROUPS
        .iter()
        .find(|g| g.level == level && g.name == group)
        .map(|g| g.colour.to_string())
}

async fn count_passes<C: ConnectionTrait>(
    db: &C,
    transect_id: Uuid,
    campaign_id: Option<Uuid>,
) -> AppResult<i64> {
    let (clause, binds): (&str, Vec<sea_orm::Value>) = match campaign_id {
        Some(campaign) => (
            "AND campaign_id = $2",
            vec![transect_id.into(), campaign.into()],
        ),
        None => ("", vec![transect_id.into()]),
    };
    let sql = format!(
        "SELECT COUNT(*)::BIGINT AS n FROM transect_pass \
         WHERE transect_id = $1 AND deleted_at IS NULL {clause}"
    );
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            &sql,
            binds,
        ))
        .await?;
    Ok(match row {
        Some(row) => row.try_get("", "n")?,
        None => 0,
    })
}
