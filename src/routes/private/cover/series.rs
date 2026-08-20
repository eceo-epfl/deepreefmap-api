//! Cover over time for one transect, pooled per survey event.
//!
//! One entry per (survey event, campaign) pair, so the console draws a series without
//! re-deriving the buckets. Passes without an event fall back to their campaign, and
//! passes with neither share one bucket, matching the pooled endpoint's semantics.
//!
//! Pooling is the same count-over-summed-denominator figure as `pooled.rs`, with the
//! per-run fraction extremes carried along as the spread of each point.

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
pub struct SeriesParams {
    /// Class hierarchy level: `fine`, `intermediate` or `coarse`.
    #[serde(default = "default_level")]
    pub level: String,
}

fn default_level() -> String {
    "coarse".to_string()
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct SeriesGroupCover {
    pub class_group: String,
    /// Share of the entry's pooled denominator, 0 to 1.
    pub fraction: f64,
    pub point_count: f64,
    /// Smallest per-run fraction among the contributing passes.
    pub min_fraction: f64,
    /// Largest per-run fraction among the contributing passes.
    pub max_fraction: f64,
    /// `#rrggbb`, from the published class group table.
    pub colour: Option<String>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct CoverSeriesEntry {
    /// The curator's survey event, or null for a campaign or unbucketed entry.
    pub group_id: Option<Uuid>,
    pub group_name: Option<String>,
    pub period_label: Option<String>,
    pub campaign_id: Option<Uuid>,
    pub campaign_name: Option<String>,
    /// Points the entry's fractions are measured over.
    pub denominator: f64,
    /// Passes that contributed a run with counts.
    pub contributing_passes: i64,
    /// Largest fraction first.
    pub groups: Vec<SeriesGroupCover>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct CoverSeries {
    pub transect_id: Uuid,
    pub level: String,
    /// Named survey events first, ordered by period label then name, then campaign
    /// buckets, then the passes with neither.
    pub entries: Vec<CoverSeriesEntry>,
}

/// Cover per survey event along one transect.
///
/// Collapses reruns to the latest succeeded run per pass, so a pass processed twice counts
/// once.
#[utoipa::path(
    get,
    path = "/transects/{id}/cover-series",
    params(("id" = Uuid, Path, description = "Transect id"), SeriesParams),
    responses(
        (status = 200, description = "Pooled cover per survey event", body = CoverSeries),
        (status = 400, description = "Unknown level"),
    ),
    tag = "cover"
)]
pub async fn cover_series(
    State(state): State<AppState>,
    Path(transect_id): Path<Uuid>,
    Query(params): Query<SeriesParams>,
) -> AppResult<Json<CoverSeries>> {
    let level = params.level;
    if !crate::contract::vocab::COVER_LEVEL
        .codes()
        .contains(&level.as_str())
    {
        return Err(AppError::BadRequest(format!(
            "Unknown level {level}: expected fine, intermediate or coarse"
        )));
    }

    // `latest` picks one run per pass, exactly as the pooled endpoint does, so the two
    // views can never disagree on which run a pass contributes.
    let sql = "\
        WITH latest AS ( \
          SELECT DISTINCT ON (r.pass_id) r.id, r.pass_id \
          FROM run_record r \
          JOIN transect_pass p ON p.id = r.pass_id \
          WHERE p.transect_id = $1 AND p.deleted_at IS NULL \
            AND r.deleted_at IS NULL AND r.status = 'succeeded' \
          ORDER BY r.pass_id, r.started_at DESC NULLS LAST, r.id DESC \
        ), \
        rows AS ( \
          SELECT p.survey_group_id, p.campaign_id, c.class_group, c.fraction, \
                 c.point_count, c.denominator, c.run_id \
          FROM cover_row c \
          JOIN latest ON latest.id = c.run_id \
          JOIN transect_pass p ON p.id = latest.pass_id \
          WHERE c.deleted_at IS NULL AND c.level = $2 AND c.estimator = 'per_pass' \
            AND c.point_count IS NOT NULL AND c.denominator IS NOT NULL \
        ), \
        totals AS ( \
          SELECT survey_group_id, campaign_id, SUM(denominator) AS denominator, \
                 COUNT(*)::BIGINT AS passes \
          FROM (SELECT DISTINCT survey_group_id, campaign_id, run_id, denominator FROM rows) d \
          GROUP BY survey_group_id, campaign_id \
        ) \
        SELECT rows.survey_group_id, rows.campaign_id, \
               g.name AS group_name, g.period_label, ca.name AS campaign_name, \
               rows.class_group, \
               SUM(rows.point_count) AS point_count, \
               MIN(rows.fraction) AS min_fraction, \
               MAX(rows.fraction) AS max_fraction, \
               totals.denominator, totals.passes AS contributing_passes \
        FROM rows \
        JOIN totals ON totals.survey_group_id IS NOT DISTINCT FROM rows.survey_group_id \
                   AND totals.campaign_id IS NOT DISTINCT FROM rows.campaign_id \
        LEFT JOIN pass_group g ON g.id = rows.survey_group_id \
        LEFT JOIN campaign ca ON ca.id = rows.campaign_id \
        GROUP BY rows.survey_group_id, rows.campaign_id, g.name, g.period_label, ca.name, \
                 rows.class_group, totals.denominator, totals.passes \
        ORDER BY (rows.survey_group_id IS NULL), g.period_label NULLS LAST, g.name, \
                 (rows.campaign_id IS NULL), ca.name, \
                 point_count DESC, rows.class_group ASC";

    let found = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            [transect_id.into(), level.clone().into()],
        ))
        .await?;

    Ok(Json(CoverSeries {
        transect_id,
        entries: fold_entries(&found, &level)?,
        level,
    }))
}

/// Fold the flat result into entries. The rows arrive entry-ordered, so a bucket change
/// starts the next entry.
fn fold_entries(found: &[sea_orm::QueryResult], level: &str) -> AppResult<Vec<CoverSeriesEntry>> {
    let mut entries: Vec<CoverSeriesEntry> = Vec::new();
    for row in found {
        let group_id: Option<Uuid> = row.try_get("", "survey_group_id")?;
        let campaign_id: Option<Uuid> = row.try_get("", "campaign_id")?;
        let same_bucket = entries
            .last()
            .is_some_and(|entry| entry.group_id == group_id && entry.campaign_id == campaign_id);
        if !same_bucket {
            entries.push(CoverSeriesEntry {
                group_id,
                group_name: row.try_get("", "group_name")?,
                period_label: row.try_get("", "period_label")?,
                campaign_id,
                campaign_name: row.try_get("", "campaign_name")?,
                denominator: row.try_get("", "denominator").unwrap_or(0.0),
                contributing_passes: row.try_get("", "contributing_passes").unwrap_or(0),
                groups: Vec::new(),
            });
        }
        let Some(entry) = entries.last_mut() else {
            continue;
        };

        let class_group: String = row.try_get("", "class_group")?;
        let point_count: f64 = row.try_get("", "point_count").unwrap_or(0.0);
        entry.groups.push(SeriesGroupCover {
            colour: super::pooled::colour_of(level, &class_group),
            class_group,
            fraction: if entry.denominator > 0.0 {
                point_count / entry.denominator
            } else {
                0.0
            },
            point_count,
            min_fraction: row.try_get("", "min_fraction").unwrap_or(0.0),
            max_fraction: row.try_get("", "max_fraction").unwrap_or(0.0),
        });
    }
    Ok(entries)
}
