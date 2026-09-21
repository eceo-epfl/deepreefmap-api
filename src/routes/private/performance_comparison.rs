//! Device-scoped comparisons over individual run observations.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Query, State},
    middleware,
};
use sea_orm::{ConnectionTrait, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use utoipa::{IntoParams, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use uuid::Uuid;

use crate::common::{AppState, auth::deny_device_crud};
use crate::error::{AppError, AppResult};

#[derive(Clone, Copy, Default, Deserialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum CompareParameter {
    #[default]
    Resolution,
    Fps,
    Batch,
    Models,
}

impl CompareParameter {
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Resolution => &["processing_width", "processing_height"],
            Self::Fps => &["fps"],
            Self::Batch => &["preprocess_batch_size"],
            Self::Models => &["mapping_backend", "segmentation_model", "model_revisions"],
        }
    }
}

#[derive(Default, Deserialize, IntoParams)]
pub struct ComparisonQuery {
    pub device_id: Option<Uuid>,
    pub preset_name: Option<String>,
    pub preset_version: Option<i32>,
    pub baseline: Option<Uuid>,
    #[serde(default)]
    #[param(inline)]
    pub parameter: CompareParameter,
    pub min_frames: Option<u32>,
    pub max_frames: Option<u32>,
    #[serde(default)]
    pub offset: usize,
}

#[derive(Clone, Debug, Serialize, ToSchema)]
pub struct Distribution {
    pub n: usize,
    pub min: Option<f64>,
    pub q1: Option<f64>,
    pub median: Option<f64>,
    pub q3: Option<f64>,
    pub max: Option<f64>,
}

fn distribution(values: impl Iterator<Item = Option<f64>>) -> Distribution {
    let mut values: Vec<_> = values
        .flatten()
        .filter(|v| v.is_finite() && *v >= 0.0)
        .collect();
    values.sort_by(f64::total_cmp);
    let percentile = |quarter: usize| {
        if values.is_empty() {
            return None;
        }
        let position = (values.len() - 1) * quarter;
        let lower = position / 4;
        let upper = (lower + 1).min(values.len() - 1);
        let fraction = [0.0, 0.25, 0.5, 0.75][position % 4];
        Some(values[lower] + (values[upper] - values[lower]) * fraction)
    };
    Distribution {
        n: values.len(),
        min: values.first().copied(),
        q1: percentile(1),
        median: percentile(2),
        q3: percentile(3),
        max: values.last().copied(),
    }
}

#[derive(Clone, Serialize, ToSchema)]
pub struct PerformanceEvidence {
    pub id: Uuid,
    pub device_id: Option<Uuid>,
    pub device_name: Option<String>,
    pub settings: Value,
    pub hardware: Value,
    pub basis: String,
    pub known: bool,
    pub status: String,
    pub recorded_at: Option<String>,
    pub frames: Option<f64>,
    pub duration_s: Option<f64>,
    pub seconds_per_frame: Option<f64>,
    pub timing_note: String,
    pub ram: Option<f64>,
    pub swap: Option<f64>,
    pub vram: Option<f64>,
}

fn peak(peaks: &Value, metric: &str) -> Option<f64> {
    peaks
        .as_object()?
        .values()
        .filter_map(|stage| stage[metric].as_f64())
        .filter(|value| value.is_finite() && *value >= 0.0)
        .max_by(f64::total_cmp)
}

fn settings_known(meta: &Value) -> bool {
    let settings = &meta["settings"];
    meta["version"] == 1
        && matches!(meta["basis"].as_str(), Some("process" | "machine"))
        && [
            "processing_width",
            "processing_height",
            "fps",
            "preprocess_batch_size",
        ]
        .iter()
        .all(|key| settings[*key].as_u64().is_some_and(|value| value > 0))
        && ["mapping_backend", "segmentation_model", "mode"]
            .iter()
            .all(|key| {
                settings[*key]
                    .as_str()
                    .is_some_and(|value| !value.is_empty())
            })
        && meta["hardware"]["total_ram_bytes"]
            .as_u64()
            .is_some_and(|value| value > 0)
}

fn timing_note(
    meta: &Value,
    completed: bool,
    frames: Option<f64>,
    duration: Option<f64>,
) -> String {
    let note = if !completed {
        "Incomplete run"
    } else if meta["version"] != 1 {
        "Eligibility not recorded"
    } else if meta["timing_complete"] != true {
        "Cached or partial execution"
    } else if frames.is_none() {
        "Frame count not recorded"
    } else if duration.is_none() {
        "Duration not recorded"
    } else {
        "Full run"
    };
    note.to_owned()
}

fn parse_observation(row: &Value) -> AppResult<PerformanceEvidence> {
    let meta = &row["performance_observation"];
    let recorded =
        meta["version"] == 1 && meta["settings"].is_object() && meta["hardware"].is_object();
    let known = recorded && settings_known(meta);
    let frames = meta["frames"]
        .as_f64()
        .filter(|v| v.is_finite() && *v > 0.0);
    let duration_s = row["run_duration_s"]
        .as_f64()
        .filter(|v| v.is_finite() && *v >= 0.0);
    let completed = matches!(row["status"].as_str(), Some("succeeded" | "completed"));
    let seconds_per_frame = if completed && meta["timing_complete"] == true {
        duration_s
            .zip(frames)
            .map(|(seconds, frames)| seconds / frames)
    } else {
        None
    };
    let id =
        serde_json::from_value(row["id"].clone()).map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(PerformanceEvidence {
        id,
        device_id: serde_json::from_value(row["device_id"].clone()).unwrap_or(None),
        device_name: row["device_name"].as_str().map(str::to_owned),
        settings: if recorded {
            meta["settings"].clone()
        } else {
            row["legacy_settings"].clone()
        },
        hardware: if recorded {
            meta["hardware"].clone()
        } else {
            Value::Null
        },
        basis: meta["basis"].as_str().unwrap_or("unknown").to_owned(),
        known,
        status: if completed {
            "completed".to_owned()
        } else {
            row["status"].as_str().unwrap_or("unknown").to_owned()
        },
        recorded_at: row["recorded_at"].as_str().map(str::to_owned),
        frames,
        duration_s,
        seconds_per_frame,
        timing_note: timing_note(meta, completed, frames, duration_s),
        ram: peak(&row["stage_peaks"], "ram_bytes"),
        swap: peak(&row["stage_peaks"], "swap_bytes"),
        vram: peak(&row["stage_peaks"], "vram_bytes"),
    })
}

fn identity(row: &PerformanceEvidence) -> String {
    json!([
        row.device_id.or(Some(row.id)),
        row.settings,
        row.hardware,
        row.basis,
        row.known
    ])
    .to_string()
}

fn comparable(
    left: &PerformanceEvidence,
    right: &PerformanceEvidence,
    parameter: CompareParameter,
) -> bool {
    if !left.known
        || !right.known
        || left.device_id.is_none()
        || left.device_id != right.device_id
        || left.hardware != right.hardware
        || left.basis != right.basis
    {
        return false;
    }
    let strip = |settings: &Value| {
        let mut settings = settings.as_object().cloned().unwrap_or_default();
        for key in parameter.keys() {
            settings.remove(*key);
        }
        settings
    };
    parameter.keys().iter().all(|key| {
        *key == "model_revisions"
            || (!left.settings[*key].is_null() && !right.settings[*key].is_null())
    }) && strip(&left.settings) == strip(&right.settings)
}

#[derive(Serialize, ToSchema)]
pub struct ConfigurationSummary {
    pub configuration: PerformanceEvidence,
    pub count: usize,
    pub completed: usize,
    pub failed: usize,
    pub workload: Distribution,
    pub stats: BTreeMap<String, Distribution>,
}

fn summarize(rows: &[&PerformanceEvidence]) -> ConfigurationSummary {
    let completed: Vec<_> = rows
        .iter()
        .filter(|row| row.status == "completed")
        .collect();
    let stats = [
        ("ram", distribution(completed.iter().map(|r| r.ram))),
        ("swap", distribution(completed.iter().map(|r| r.swap))),
        ("vram", distribution(completed.iter().map(|r| r.vram))),
        (
            "seconds_per_frame",
            distribution(completed.iter().map(|r| r.seconds_per_frame)),
        ),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    ConfigurationSummary {
        configuration: rows[0].clone(),
        count: rows.len(),
        completed: completed.len(),
        failed: rows.iter().filter(|row| row.status == "failed").count(),
        workload: distribution(rows.iter().map(|row| row.frames)),
        stats,
    }
}

const OBSERVATIONS_SQL: &str = "SELECT jsonb_build_object(
    'id', r.id, 'device_id', r.device_id, 'device_name', d.name, 'status', r.status,
    'recorded_at', r.started_at, 'run_duration_s', r.run_duration_s,
    'stage_peaks', r.stage_peaks, 'performance_observation', r.performance_observation,
    'legacy_settings', jsonb_build_object('processing_width', r.processing_width,
        'processing_height', r.processing_height, 'fps', r.fps,
        'preprocess_batch_size', r.preprocess_batch_size, 'mapping_backend', r.mapping_backend,
        'segmentation_model', r.segmentation_model, 'preset_hash', r.preset_hash)
    ) AS observation FROM run_record r LEFT JOIN device d ON d.id = r.device_id
    WHERE r.deleted_at IS NULL AND ($1::uuid IS NULL OR r.device_id = $1)
        AND ($2::text IS NULL OR r.preset_name = $2)
        AND ($3::int IS NULL OR r.preset_version = $3)
        AND (r.stage_peaks IS NOT NULL OR r.performance_observation IS NOT NULL)
    ORDER BY r.started_at DESC NULLS LAST, r.id";

async fn observations(
    state: &AppState,
    query: &ComparisonQuery,
) -> AppResult<Vec<PerformanceEvidence>> {
    if query
        .min_frames
        .zip(query.max_frames)
        .is_some_and(|(min, max)| min > max)
    {
        return Err(AppError::BadRequest(
            "Minimum frames exceeds maximum frames".into(),
        ));
    }
    let rows = state
        .db
        .query_all_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            OBSERVATIONS_SQL,
            vec![
                query.device_id.into(),
                query.preset_name.clone().into(),
                query.preset_version.into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|row| parse_observation(&row.try_get::<Value>("", "observation")?))
        .collect()
}

fn in_workload(row: &PerformanceEvidence, query: &ComparisonQuery) -> bool {
    if query.min_frames.is_none() && query.max_frames.is_none() {
        return true;
    }
    row.frames.is_some_and(|frames| {
        query.min_frames.is_none_or(|min| frames >= f64::from(min))
            && query.max_frames.is_none_or(|max| frames <= f64::from(max))
    })
}

fn baseline<'a>(
    rows: &'a [PerformanceEvidence],
    query: &ComparisonQuery,
) -> Option<&'a PerformanceEvidence> {
    match query.baseline {
        Some(id) => rows.iter().find(|row| row.id == id),
        None => rows.first(),
    }
}

#[derive(Serialize, ToSchema)]
pub struct PerformanceComparison {
    pub configurations: Vec<PerformanceEvidence>,
    pub baseline: Option<ConfigurationSummary>,
    pub alternatives: Vec<ConfigurationSummary>,
}

#[utoipa::path(get, path = "/performance/comparison", params(ComparisonQuery),
    responses((status = 200, body = PerformanceComparison)), tag = "performance")]
async fn comparison(
    State(state): State<AppState>,
    Query(query): Query<ComparisonQuery>,
) -> AppResult<Json<PerformanceComparison>> {
    let rows = observations(&state, &query).await?;
    let selected = baseline(&rows, &query);
    let mut groups: BTreeMap<String, Vec<&PerformanceEvidence>> = BTreeMap::new();
    for row in &rows {
        groups.entry(identity(row)).or_default().push(row);
    }
    let mut configurations: Vec<_> = groups.values().map(|members| members[0].clone()).collect();
    configurations.sort_by(|a, b| b.recorded_at.cmp(&a.recorded_at).then(a.id.cmp(&b.id)));
    let selected_key = selected.map(identity);
    let mut summary = None;
    let mut alternatives = Vec::new();
    for (key, members) in groups {
        let matching: Vec<_> = members
            .into_iter()
            .filter(|row| in_workload(row, &query))
            .collect();
        if matching.is_empty() {
            continue;
        }
        if Some(&key) == selected_key.as_ref() {
            summary = Some(summarize(&matching));
        } else if selected.is_some_and(|base| comparable(base, matching[0], query.parameter)) {
            alternatives.push(summarize(&matching));
        }
    }
    Ok(Json(PerformanceComparison {
        configurations,
        baseline: summary,
        alternatives,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct PerformanceEvidencePage {
    pub rows: Vec<PerformanceEvidence>,
    pub total: usize,
    pub offset: usize,
}

#[utoipa::path(get, path = "/performance/evidence", params(ComparisonQuery),
    responses((status = 200, body = PerformanceEvidencePage)), tag = "performance")]
async fn evidence(
    State(state): State<AppState>,
    Query(query): Query<ComparisonQuery>,
) -> AppResult<Json<PerformanceEvidencePage>> {
    let rows = observations(&state, &query).await?;
    let selected_key = baseline(&rows, &query).map(identity);
    let matching: Vec<_> = rows
        .into_iter()
        .filter(|row| Some(identity(row)) == selected_key && in_workload(row, &query))
        .collect();
    let total = matching.len();
    let rows = matching.into_iter().skip(query.offset).take(50).collect();
    Ok(Json(PerformanceEvidencePage {
        rows,
        total,
        offset: query.offset,
    }))
}

pub fn router(state: &AppState) -> OpenApiRouter {
    OpenApiRouter::new()
        .routes(utoipa_axum::routes!(comparison))
        .routes(utoipa_axum::routes!(evidence))
        .layer(middleware::from_fn(deny_device_crud))
        .with_state(state.clone())
}

#[cfg(test)]
#[path = "tests/performance_comparison.rs"]
mod tests;
