//! The replicated table set, described once.
//!
//! Push and pull are generic over these descriptors, and `contract/sync-contract.json`
//! publishes them as the sync contract.
//!
//! Column names here are interpolated into SQL; values always bind as parameters.
//! Nothing outside this file may contribute a column name.

use crate::error::{AppError, AppResult};
use sea_orm::Value;

/// How a column's JSON crosses into Postgres. Typed, because inferring from the JSON
/// would bind `1` as an integer in a float column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ColumnKind {
    Uuid,
    Text,
    Float,
    Int,
    BigInt,
    Bool,
    Timestamp,
    Date,
    Json,
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct ColumnSpec {
    pub name: &'static str,
    pub kind: ColumnKind,
    /// Whether a document may omit the column or send it null.
    pub nullable: bool,
}

const fn col(name: &'static str, kind: ColumnKind) -> ColumnSpec {
    ColumnSpec {
        name,
        kind,
        nullable: true,
    }
}

const fn required(name: &'static str, kind: ColumnKind) -> ColumnSpec {
    ColumnSpec {
        name,
        kind,
        nullable: false,
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct TableSpec {
    /// The section key in a sync document.
    pub section: &'static str,
    pub table: &'static str,
    /// Entity-specific columns. The shared sync columns are appended by
    /// [`TableSpec::columns`], so no descriptor repeats them.
    pub own_columns: &'static [ColumnSpec],
}

/// Columns every replicated row carries. `updated_at` is the client's clock and what
/// conflicts resolve on. `server_seq` is absent: clients must not set their own
/// position in anyone else's pull order. Provenance is `device_id` alone: rows are
/// attributed to the laptop that pushed them, never to a person.
const SYNC_COLUMNS: &[ColumnSpec] = &[
    required("created_at", ColumnKind::Timestamp),
    required("updated_at", ColumnKind::Timestamp),
    col("deleted_at", ColumnKind::Timestamp),
    col("device_id", ColumnKind::Uuid),
];

impl TableSpec {
    /// Every column a document may carry for this table, `id` first.
    #[must_use]
    pub fn columns(&self) -> Vec<ColumnSpec> {
        let mut out = vec![required("id", ColumnKind::Uuid)];
        out.extend_from_slice(self.own_columns);
        out.extend_from_slice(SYNC_COLUMNS);
        out
    }
}

/// The replicated tables, in foreign-key order, so a document applies top to bottom.
///
/// The desktop application's batch, batch-item and notification tables are excluded:
/// they are one workstation's queue and message log, not survey facts.
pub const TABLES: &[TableSpec] = &[
    TableSpec {
        section: "sites",
        table: "site",
        own_columns: &[
            required("name", ColumnKind::Text),
            col("country", ColumnKind::Text),
            col("region", ColumnKind::Text),
            required("description", ColumnKind::Text),
            col("latitude", ColumnKind::Float),
            col("longitude", ColumnKind::Float),
        ],
    },
    TableSpec {
        section: "campaigns",
        table: "campaign",
        own_columns: &[
            required("name", ColumnKind::Text),
            col("begin_date", ColumnKind::Date),
            col("end_date", ColumnKind::Date),
            required("description", ColumnKind::Text),
        ],
    },
    TableSpec {
        section: "transects",
        table: "transect",
        own_columns: &[
            col("site_id", ColumnKind::Uuid),
            required("name", ColumnKind::Text),
            required("description", ColumnKind::Text),
            required("start_lat", ColumnKind::Float),
            required("start_lon", ColumnKind::Float),
            col("start_accuracy_m", ColumnKind::Float),
            required("end_lat", ColumnKind::Float),
            required("end_lon", ColumnKind::Float),
            col("end_accuracy_m", ColumnKind::Float),
            col("length_m", ColumnKind::Float),
            col("depth_m", ColumnKind::Float),
        ],
    },
    TableSpec {
        section: "videos",
        table: "video_asset",
        own_columns: &[
            col("hash", ColumnKind::Text),
            required("file_name", ColumnKind::Text),
            col("size_bytes", ColumnKind::BigInt),
            col("duration_s", ColumnKind::Float),
            col("fps", ColumnKind::Float),
            col("width", ColumnKind::Int),
            col("height", ColumnKind::Int),
            col("codec", ColumnKind::Text),
            col("captured_at", ColumnKind::Timestamp),
            col("captured_source", ColumnKind::Text),
            required("gravity", ColumnKind::Text),
            required("gps", ColumnKind::Text),
        ],
    },
    // `survey_group_id` is deliberately absent: a curator assigns it in the console,
    // and a device re-pushing its pass must never clobber that grouping.
    TableSpec {
        section: "passes",
        table: "transect_pass",
        own_columns: &[
            col("transect_id", ColumnKind::Uuid),
            col("campaign_id", ColumnKind::Uuid),
            required("begin_s", ColumnKind::Float),
            required("end_s", ColumnKind::Float),
            col("direction", ColumnKind::Text),
            required("upside_down", ColumnKind::Bool),
            required("label", ColumnKind::Text),
            required("notes", ColumnKind::Text),
            col("quality", ColumnKind::Text),
        ],
    },
    TableSpec {
        section: "pass_videos",
        table: "pass_video",
        own_columns: &[
            required("pass_id", ColumnKind::Uuid),
            required("video_id", ColumnKind::Uuid),
            required("ordinal", ColumnKind::Int),
        ],
    },
    TableSpec {
        section: "runs",
        table: "run_record",
        own_columns: &[
            required("pass_id", ColumnKind::Uuid),
            required("status", ColumnKind::Text),
            col("started_at", ColumnKind::Timestamp),
            col("finished_at", ColumnKind::Timestamp),
            required("error", ColumnKind::Text),
            required("run_dir_name", ColumnKind::Text),
            col("gui_version", ColumnKind::Text),
            col("library_version", ColumnKind::Text),
            col("segmentation_model", ColumnKind::Text),
            col("mapping_backend", ColumnKind::Text),
            col("taxonomy_version", ColumnKind::Int),
            col("taxonomy_hash", ColumnKind::Text),
            col("model_revisions", ColumnKind::Json),
            col("preset_name", ColumnKind::Text),
            col("preset_deviations", ColumnKind::Json),
            col("preset_version", ColumnKind::Int),
            col("preset_hash", ColumnKind::Text),
            col("run_duration_s", ColumnKind::Float),
            col("stage_durations", ColumnKind::Json),
            col("stage_peaks", ColumnKind::Json),
        ],
    },
    TableSpec {
        section: "cover_rows",
        table: "cover_row",
        own_columns: &[
            required("run_id", ColumnKind::Uuid),
            required("level", ColumnKind::Text),
            required("class_group", ColumnKind::Text),
            required("estimator", ColumnKind::Text),
            required("fraction", ColumnKind::Float),
            col("point_count", ColumnKind::Float),
            col("denominator", ColumnKind::Float),
            col("metric_source", ColumnKind::Text),
        ],
    },
    // Appended last so existing clients' section order is undisturbed. Pull only:
    // presets are server-defined, and a device never authors one.
    TableSpec {
        section: "presets",
        table: "preset",
        own_columns: &[
            required("name", ColumnKind::Text),
            required("version", ColumnKind::Int),
            required("settings", ColumnKind::Json),
            required("description", ColumnKind::Text),
        ],
    },
];

/// Highest contract version this server speaks. A document declaring anything but the
/// version negotiated for its exchange is refused outright: parsing under the wrong
/// version writes plausible wrong rows instead of failing.
pub const CONTRACT_VERSION: u32 = 1;

/// Oldest contract version this server still reads. Together with [`CONTRACT_VERSION`] it
/// is the range a client negotiates against.
pub const MIN_CONTRACT_VERSION: u32 = 1;

const _: () = assert!(
    MIN_CONTRACT_VERSION <= CONTRACT_VERSION,
    "the server's contract range must contain at least one version"
);

/// Sections a client may download. Everything else is upload only.
///
/// Sync is additive: a site, campaign or transect is defined on either side, and the
/// rest is a client's own record of what it processed, so sending it back down would
/// only hand a device its own work. Presets travel downwards only, since the server
/// defines them.
pub const CLIENT_PULL_SECTIONS: &[&str] = &["sites", "campaigns", "transects", "presets"];

/// Sections a device may author rows in. The rest are read on a push, never written.
///
/// The desktop application has no site or campaign picker, so it creates neither.
pub const CLIENT_PUSH_SECTIONS: &[&str] = &[
    "transects",
    "videos",
    "passes",
    "pass_videos",
    "runs",
    "cover_rows",
];

/// Every section name, in the order a document applies.
#[must_use]
pub fn sections() -> Vec<&'static str> {
    TABLES.iter().map(|spec| spec.section).collect()
}

/// The published description of every table, as `contract/sync-contract.json` renders it.
#[must_use]
pub fn table_documents() -> Vec<serde_json::Value> {
    TABLES
        .iter()
        .map(|spec| {
            serde_json::json!({
                "section": spec.section,
                "table": spec.table,
                "columns": spec.columns(),
            })
        })
        .collect()
}

#[must_use]
pub fn table_for_section(section: &str) -> Option<&'static TableSpec> {
    TABLES.iter().find(|t| t.section == section)
}

/// Convert one JSON value into a bindable parameter of the column's declared type.
///
/// Rejects rather than coerces: a string where a number belongs is a client bug.
pub fn to_value(spec: &ColumnSpec, raw: Option<&serde_json::Value>) -> AppResult<Value> {
    let json = match raw {
        None | Some(serde_json::Value::Null) => {
            if spec.nullable {
                return Ok(null_value(spec.kind));
            }
            return Err(AppError::BadRequest(format!(
                "{} must not be null",
                spec.name
            )));
        }
        Some(value) => value,
    };

    let bad = |expected: &str| {
        AppError::BadRequest(format!("{} must be {expected}, got {json}", spec.name))
    };

    Ok(match spec.kind {
        ColumnKind::Uuid => {
            let text = json.as_str().ok_or_else(|| bad("a UUID string"))?;
            let parsed: uuid::Uuid = text.parse().map_err(|_| bad("a valid UUID"))?;
            Value::from(parsed)
        }
        ColumnKind::Text => Value::from(json.as_str().ok_or_else(|| bad("a string"))?),
        ColumnKind::Float => Value::from(json.as_f64().ok_or_else(|| bad("a number"))?),
        ColumnKind::Int => {
            let n = json.as_i64().ok_or_else(|| bad("an integer"))?;
            Value::from(i32::try_from(n).map_err(|_| bad("a 32-bit integer"))?)
        }
        ColumnKind::BigInt => Value::from(json.as_i64().ok_or_else(|| bad("an integer"))?),
        ColumnKind::Bool => Value::from(json.as_bool().ok_or_else(|| bad("a boolean"))?),
        ColumnKind::Timestamp => {
            let text = json.as_str().ok_or_else(|| bad("an RFC 3339 timestamp"))?;
            let parsed = chrono::DateTime::parse_from_rfc3339(text)
                .map_err(|_| bad("an RFC 3339 timestamp"))?
                .with_timezone(&chrono::Utc);
            Value::from(parsed)
        }
        ColumnKind::Date => {
            let text = json.as_str().ok_or_else(|| bad("a YYYY-MM-DD date"))?;
            let parsed = chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                .map_err(|_| bad("a YYYY-MM-DD date"))?;
            Value::from(parsed)
        }
        ColumnKind::Json => Value::from(json.clone()),
    })
}

/// A typed SQL null, so Postgres can infer the parameter type on an all-null bind.
fn null_value(kind: ColumnKind) -> Value {
    match kind {
        ColumnKind::Uuid => Value::Uuid(None),
        ColumnKind::Text => Value::String(None),
        ColumnKind::Float => Value::Double(None),
        ColumnKind::Int => Value::Int(None),
        ColumnKind::BigInt => Value::BigInt(None),
        ColumnKind::Bool => Value::Bool(None),
        ColumnKind::Timestamp => Value::ChronoDateTimeUtc(None),
        ColumnKind::Date => Value::ChronoDate(None),
        ColumnKind::Json => Value::Json(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sections_lists_every_table_in_order() {
        let listed = sections();
        assert_eq!(listed.len(), TABLES.len());
        assert_eq!(listed[0], "sites");
        assert_eq!(listed[listed.len() - 1], "presets");
    }

    /// A section a device can neither author nor download would be unreachable to it.
    #[test]
    fn test_every_section_is_reachable_from_a_device() {
        for spec in TABLES {
            assert!(
                CLIENT_PUSH_SECTIONS.contains(&spec.section)
                    || CLIENT_PULL_SECTIONS.contains(&spec.section),
                "{} is neither pushable nor pullable",
                spec.section
            );
        }
    }

    /// Reference-only sections are read on a push, so they must be pullable or a device
    /// could never hold the ancestor it is asked to send.
    #[test]
    fn test_a_section_a_device_cannot_author_is_one_it_can_pull() {
        for spec in TABLES {
            if CLIENT_PUSH_SECTIONS.contains(&spec.section) {
                continue;
            }
            assert!(
                CLIENT_PULL_SECTIONS.contains(&spec.section),
                "{} is reference-only on a push but never served on a pull",
                spec.section
            );
        }
    }

    #[test]
    fn test_sections_are_unique() {
        let mut seen = std::collections::HashSet::new();
        for spec in TABLES {
            assert!(
                seen.insert(spec.section),
                "duplicate section {}",
                spec.section
            );
        }
    }

    #[test]
    fn test_no_descriptor_redeclares_sync_column() {
        for spec in TABLES {
            for own in spec.own_columns {
                assert!(
                    !SYNC_COLUMNS.iter().any(|s| s.name == own.name) && own.name != "id",
                    "{} redeclares {}",
                    spec.table,
                    own.name
                );
            }
        }
    }

    #[test]
    fn test_every_table_carries_the_columns_a_document_needs() {
        assert_eq!(TABLES.len(), 9);
        for spec in TABLES {
            let names: Vec<&str> = spec.columns().iter().map(|c| c.name).collect();
            for needed in ["id", "updated_at", "deleted_at"] {
                assert!(names.contains(&needed), "{} lacks {needed}", spec.table);
            }
        }
    }

    #[test]
    fn test_server_seq_not_client_writable() {
        for spec in TABLES {
            assert!(
                !spec.columns().iter().any(|c| c.name == "server_seq"),
                "{} exposes server_seq to clients",
                spec.table
            );
        }
    }

    #[test]
    fn test_required_column_rejects_null_and_absence() {
        let spec = required("name", ColumnKind::Text);
        assert!(to_value(&spec, None).is_err());
        assert!(to_value(&spec, Some(&serde_json::Value::Null)).is_err());
    }

    #[test]
    fn test_wrong_type_is_rejected() {
        let float = col("length_m", ColumnKind::Float);
        assert!(to_value(&float, Some(&serde_json::json!("12"))).is_err());
        let uuid = col("site_id", ColumnKind::Uuid);
        assert!(to_value(&uuid, Some(&serde_json::json!("not-a-uuid"))).is_err());
        let ts = col("deleted_at", ColumnKind::Timestamp);
        assert!(to_value(&ts, Some(&serde_json::json!("yesterday"))).is_err());
    }

    #[test]
    fn test_absent_nullable_binds_typed_null() {
        let spec = col("length_m", ColumnKind::Float);
        assert_eq!(to_value(&spec, None).unwrap(), Value::Double(None));
    }
}
