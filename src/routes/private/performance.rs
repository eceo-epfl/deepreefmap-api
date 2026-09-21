//! Fleet-wide resource statistics, for the console's Performance page.
//!
//! `stage_peaks` is detail-only on `/api/runs`, so comparing devices or presets would
//! take one request per run. This aggregate folds every run that carries peaks into
//! one row per device, model combination and processing configuration, relatable to
//! the preset it ran under.
//!
//! Two levels of aggregation, and the order between them is the whole point. A run's
//! observation for a metric is the largest value across that run's stages, its peak.
//! The published figures are then computed across runs over those per-run peaks, so
//! ten runs contribute ten observations however many stages each of them recorded.

use axum::{Json, extract::State, middleware};
use sea_orm::{ConnectionTrait, Statement};
use tower_http::limit::RequestBodyLimitLayer;
use utoipa::ToSchema;
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use crate::common::AppState;
use crate::common::auth::deny_device_crud;
use crate::error::AppResult;
use crate::routes::CRUD_BODY_LIMIT;

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PerformanceGroup {
    /// Null when the runs arrived under an interactive login rather than a device
    /// token: provenance binds from the credential, and a human push carries no device.
    pub device_id: Option<Uuid>,
    pub device_name: Option<String>,
    /// From the device's current profile, as are the two totals below: what the
    /// hardware is now, not what it was when the runs happened.
    pub gpu_name: Option<String>,
    pub total_ram_bytes: Option<i64>,
    pub total_vram_bytes: Option<i64>,
    pub preset_name: Option<String>,
    pub preset_version: Option<i32>,
    pub preset_hash: Option<String>,
    pub segmentation_model: Option<String>,
    pub mapping_backend: Option<String>,
    /// Processing configuration, part of the group key: resolution and fps set the
    /// memory regime, batch size gates VRAM. All null for a run pushed by a build that
    /// did not report them.
    pub processing_width: Option<i32>,
    pub processing_height: Option<i32>,
    pub fps: Option<i32>,
    pub preprocess_batch_size: Option<i32>,
    /// Every run in the group that recorded peaks, whatever its status. A run whose
    /// `stage_peaks` is absent, empty, or something other than a map of stages is not
    /// in the group at all.
    pub run_count: i64,
    /// The failed subset of `run_count`.
    pub failed_count: i64,
    /// Mean of the per-run RAM peaks. Every metric below carries the same five figures.
    pub ram_mean_bytes: Option<f64>,
    /// Sample standard deviation, null below two observations.
    pub ram_std_bytes: Option<f64>,
    pub ram_min_bytes: Option<i64>,
    pub ram_max_bytes: Option<i64>,
    /// Runs that observed a RAM peak. Counted per metric rather than per group, so it
    /// can sit below `run_count`: a machine with no discrete GPU reports no VRAM at all,
    /// and a run that reported junk is a run with that metric unobserved.
    pub ram_n: i64,
    pub swap_mean_bytes: Option<f64>,
    pub swap_std_bytes: Option<f64>,
    pub swap_min_bytes: Option<i64>,
    pub swap_max_bytes: Option<i64>,
    pub swap_n: i64,
    pub vram_mean_bytes: Option<f64>,
    pub vram_std_bytes: Option<f64>,
    pub vram_min_bytes: Option<i64>,
    pub vram_max_bytes: Option<i64>,
    pub vram_n: i64,
    /// Wall-clock seconds, over the runs that report a duration.
    pub duration_mean_s: Option<f64>,
    pub duration_std_s: Option<f64>,
    pub duration_min_s: Option<f64>,
    pub duration_max_s: Option<f64>,
    pub duration_n: i64,
    /// Latest `started_at` in the group.
    pub last_run_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, serde::Serialize, ToSchema)]
pub struct PerformanceSummary {
    pub groups: Vec<PerformanceGroup>,
}

/// SQL rather than fetch-and-fold: `jsonb_each` keeps the per-stage max on the database,
/// so the response stays one row per group however many runs exist. The lateral join
/// stays guarded even though the filter below admits objects alone, because a `WHERE`
/// applies after the join unless the planner chooses to push it down, and `jsonb_each`
/// raises on a scalar.
///
/// That filter sits in `per_run` rather than outside it so a run that recorded nothing
/// never reaches `COUNT(*)`. It tests the JSON rather than the column, because a device
/// that sent `{}` or a scalar said exactly what one omitting the column said, and
/// admitting either would put a run the figures say nothing about into `run_count` and
/// into `failed_count`.
///
/// `STDDEV_SAMP` rather than `STDDEV_POP`: these runs are a sample of what the
/// configuration costs, not the population of everything it could ever cost. It returns
/// null for a single observation, which is the honest answer.
const SUMMARY_SQL: &str = "\
    WITH per_run AS ( \
      SELECT r.id, r.device_id, r.preset_name, r.preset_version, r.preset_hash, \
             r.segmentation_model, r.mapping_backend, r.processing_width, \
             r.processing_height, r.fps, r.preprocess_batch_size, r.status, \
             r.run_duration_s, r.started_at, \
             MAX(CASE WHEN jsonb_typeof(stage.value->'ram_bytes') = 'number' \
                      THEN (stage.value->>'ram_bytes')::DOUBLE PRECISION END) AS ram_bytes, \
             MAX(CASE WHEN jsonb_typeof(stage.value->'swap_bytes') = 'number' \
                      THEN (stage.value->>'swap_bytes')::DOUBLE PRECISION END) AS swap_bytes, \
             MAX(CASE WHEN jsonb_typeof(stage.value->'vram_bytes') = 'number' \
                      THEN (stage.value->>'vram_bytes')::DOUBLE PRECISION END) AS vram_bytes \
      FROM run_record r \
      LEFT JOIN LATERAL jsonb_each( \
        CASE WHEN jsonb_typeof(r.stage_peaks) = 'object' THEN r.stage_peaks END \
      ) AS stage ON TRUE \
      WHERE r.deleted_at IS NULL AND jsonb_typeof(r.stage_peaks) = 'object' \
            AND r.stage_peaks <> '{}'::jsonb \
      GROUP BY r.id \
    ) \
    SELECT per_run.device_id, d.name AS device_name, \
           d.system_profile->'gpu'->>'name' AS gpu_name, \
           CASE WHEN jsonb_typeof(d.system_profile->'total_ram_bytes') = 'number' \
                THEN (d.system_profile->>'total_ram_bytes')::DOUBLE PRECISION END \
               AS total_ram_bytes, \
           CASE WHEN jsonb_typeof(d.system_profile->'gpu'->'total_vram_bytes') = 'number' \
                THEN (d.system_profile->'gpu'->>'total_vram_bytes')::DOUBLE PRECISION END \
               AS total_vram_bytes, \
           per_run.preset_name, per_run.preset_version, per_run.preset_hash, \
           per_run.segmentation_model, per_run.mapping_backend, \
           per_run.processing_width, per_run.processing_height, per_run.fps, \
           per_run.preprocess_batch_size, \
           COUNT(*)::BIGINT AS run_count, \
           (COUNT(*) FILTER (WHERE per_run.status = 'failed'))::BIGINT AS failed_count, \
           AVG(per_run.ram_bytes) AS ram_mean_bytes, \
           STDDEV_SAMP(per_run.ram_bytes) AS ram_std_bytes, \
           MIN(per_run.ram_bytes) AS ram_min_bytes, \
           MAX(per_run.ram_bytes) AS ram_max_bytes, \
           COUNT(per_run.ram_bytes)::BIGINT AS ram_n, \
           AVG(per_run.swap_bytes) AS swap_mean_bytes, \
           STDDEV_SAMP(per_run.swap_bytes) AS swap_std_bytes, \
           MIN(per_run.swap_bytes) AS swap_min_bytes, \
           MAX(per_run.swap_bytes) AS swap_max_bytes, \
           COUNT(per_run.swap_bytes)::BIGINT AS swap_n, \
           AVG(per_run.vram_bytes) AS vram_mean_bytes, \
           STDDEV_SAMP(per_run.vram_bytes) AS vram_std_bytes, \
           MIN(per_run.vram_bytes) AS vram_min_bytes, \
           MAX(per_run.vram_bytes) AS vram_max_bytes, \
           COUNT(per_run.vram_bytes)::BIGINT AS vram_n, \
           AVG(per_run.run_duration_s) AS duration_mean_s, \
           STDDEV_SAMP(per_run.run_duration_s) AS duration_std_s, \
           MIN(per_run.run_duration_s) AS duration_min_s, \
           MAX(per_run.run_duration_s) AS duration_max_s, \
           COUNT(per_run.run_duration_s)::BIGINT AS duration_n, \
           MAX(per_run.started_at) AS last_run_at \
    FROM per_run \
    LEFT JOIN device d ON d.id = per_run.device_id \
    GROUP BY per_run.device_id, d.id, per_run.preset_name, per_run.preset_version, \
             per_run.preset_hash, per_run.segmentation_model, per_run.mapping_backend, \
             per_run.processing_width, per_run.processing_height, per_run.fps, \
             per_run.preprocess_batch_size \
    ORDER BY d.name NULLS LAST, per_run.device_id NULLS LAST, \
             per_run.segmentation_model NULLS LAST, \
             per_run.mapping_backend NULLS LAST, per_run.preset_name NULLS LAST, \
             per_run.preset_version NULLS LAST, per_run.preset_hash NULLS LAST, \
             per_run.processing_width NULLS LAST, per_run.processing_height NULLS LAST, \
             per_run.fps NULLS LAST, per_run.preprocess_batch_size NULLS LAST";

/// Resource statistics per device, preset, model combination and processing
/// configuration, the grain the desktop application keys its own history by.
///
/// One run contributes one observation per metric: the largest value across that run's
/// stages. Mean, sample standard deviation, min, max and n are then taken across the
/// group's runs over those per-run peaks. The standard deviation is null below two
/// observations, and n is per metric, so a fleet of machines without discrete GPUs
/// reports a `vram_n` of zero beside a full `ram_n`.
///
/// Covers the non-deleted runs that recorded per-stage peaks, and only those. A failed
/// run counts once it recorded them, which is the point: an out-of-memory run's peaks are
/// exactly the interesting ones. A run that recorded none contributes nothing at all, not
/// even to `run_count`: a run synced by a build that never reported peaks, one that died
/// before a single stage finished, and one whose `stage_peaks` arrived empty or as
/// something other than a map of stages are all absent here rather than counted as runs
/// with nothing to show. Revoked devices keep their history for the same reason failed
/// runs do. Within a stage map `stage_peaks` is untrusted JSON, so a non-numeric or
/// absent value is ignored rather than refused, and the run still counts with that metric
/// unobserved.
///
/// The device hardware figures (`gpu_name`, `total_ram_bytes`, `total_vram_bytes`) come
/// from the device's current profile, not from its profile when the runs happened, so a
/// machine that has since been upgraded reports its new hardware against its old runs.
#[utoipa::path(
    get,
    path = "/performance/summary",
    responses(
        (status = 200, description = "One row per device, preset, model combination and processing configuration", body = PerformanceSummary),
    ),
    tag = "performance"
)]
pub async fn performance_summary(
    State(state): State<AppState>,
) -> AppResult<Json<PerformanceSummary>> {
    let found = state
        .db
        .query_all_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            SUMMARY_SQL,
        ))
        .await?;

    Ok(Json(PerformanceSummary {
        groups: fold_groups(&found)?,
    }))
}

fn fold_groups(found: &[sea_orm::QueryResult]) -> AppResult<Vec<PerformanceGroup>> {
    // Byte figures travel as DOUBLE PRECISION and land here, so an absurd reported
    // value saturates instead of failing a BIGINT cast in SQL. The truncation the
    // lint warns about is that saturation. Means and deviations stay floating point:
    // the average of two byte counts is not itself a byte count.
    #[allow(clippy::cast_possible_truncation)]
    let as_bytes = |value: Option<f64>| value.map(|v| v as i64);

    let mut groups = Vec::with_capacity(found.len());
    for row in found {
        groups.push(PerformanceGroup {
            device_id: row.try_get("", "device_id")?,
            device_name: row.try_get("", "device_name")?,
            gpu_name: row.try_get("", "gpu_name")?,
            total_ram_bytes: as_bytes(row.try_get("", "total_ram_bytes")?),
            total_vram_bytes: as_bytes(row.try_get("", "total_vram_bytes")?),
            preset_name: row.try_get("", "preset_name")?,
            preset_version: row.try_get("", "preset_version")?,
            preset_hash: row.try_get("", "preset_hash")?,
            segmentation_model: row.try_get("", "segmentation_model")?,
            mapping_backend: row.try_get("", "mapping_backend")?,
            processing_width: row.try_get("", "processing_width")?,
            processing_height: row.try_get("", "processing_height")?,
            fps: row.try_get("", "fps")?,
            preprocess_batch_size: row.try_get("", "preprocess_batch_size")?,
            run_count: row.try_get("", "run_count")?,
            failed_count: row.try_get("", "failed_count")?,
            ram_mean_bytes: row.try_get("", "ram_mean_bytes")?,
            ram_std_bytes: row.try_get("", "ram_std_bytes")?,
            ram_min_bytes: as_bytes(row.try_get("", "ram_min_bytes")?),
            ram_max_bytes: as_bytes(row.try_get("", "ram_max_bytes")?),
            ram_n: row.try_get("", "ram_n")?,
            swap_mean_bytes: row.try_get("", "swap_mean_bytes")?,
            swap_std_bytes: row.try_get("", "swap_std_bytes")?,
            swap_min_bytes: as_bytes(row.try_get("", "swap_min_bytes")?),
            swap_max_bytes: as_bytes(row.try_get("", "swap_max_bytes")?),
            swap_n: row.try_get("", "swap_n")?,
            vram_mean_bytes: row.try_get("", "vram_mean_bytes")?,
            vram_std_bytes: row.try_get("", "vram_std_bytes")?,
            vram_min_bytes: as_bytes(row.try_get("", "vram_min_bytes")?),
            vram_max_bytes: as_bytes(row.try_get("", "vram_max_bytes")?),
            vram_n: row.try_get("", "vram_n")?,
            duration_mean_s: row.try_get("", "duration_mean_s")?,
            duration_std_s: row.try_get("", "duration_std_s")?,
            duration_min_s: row.try_get("", "duration_min_s")?,
            duration_max_s: row.try_get("", "duration_max_s")?,
            duration_n: row.try_get("", "duration_n")?,
            last_run_at: row.try_get("", "last_run_at")?,
        });
    }
    Ok(groups)
}

/// Read view for people: devices report peaks through sync, they do not browse them.
pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(performance_summary))
        .layer(middleware::from_fn(deny_device_crud))
        .layer(RequestBodyLimitLayer::new(CRUD_BODY_LIMIT))
        .with_state(state.clone())
}
