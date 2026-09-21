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
    /// The contract version that introduced the column. A client negotiated below it
    /// neither receives it nor writes it.
    pub since: u32,
    /// Travels downwards only: the server projects it, a push never writes it.
    pub server_owned: bool,
    /// The database computes it from other columns of the same row. A device may
    /// still send it, and it stands where the sources are absent, but it never
    /// enters a patch: two devices agreeing on the ends and disagreeing on the
    /// derived figure are not in conflict.
    pub derived: bool,
}

impl ColumnSpec {
    /// Marks a column added after the first release. Unused until the contract grows.
    #[cfg_attr(not(test), allow(dead_code))]
    const fn since(mut self, version: u32) -> Self {
        self.since = version;
        self
    }

    const fn server_owned(mut self) -> Self {
        self.server_owned = true;
        self
    }

    const fn derived(mut self) -> Self {
        self.derived = true;
        self
    }
}

const fn col(name: &'static str, kind: ColumnKind) -> ColumnSpec {
    ColumnSpec {
        name,
        kind,
        nullable: true,
        since: 1,
        server_owned: false,
        derived: false,
    }
}

const fn required(name: &'static str, kind: ColumnKind) -> ColumnSpec {
    ColumnSpec {
        name,
        kind,
        nullable: false,
        since: 1,
        server_owned: false,
        derived: false,
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
    /// Whether a curator validates rows of this table, which appends the
    /// [`VALIDATION_COLUMNS`].
    pub curated: bool,
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

/// The projection of a console entry that validated the row. Down only.
const VALIDATION_COLUMNS: &[ColumnSpec] = &[
    col("validated_at", ColumnKind::Timestamp).server_owned(),
    col("validated_by", ColumnKind::Text).server_owned(),
];

impl TableSpec {
    /// Every column a document may carry for this table at the newest contract, `id`
    /// first.
    #[must_use]
    pub fn columns(&self) -> Vec<ColumnSpec> {
        self.columns_at(CONTRACT_VERSION)
    }

    /// The columns a client negotiated to `agreed` knows, `id` first.
    #[must_use]
    pub fn columns_at(&self, agreed: u32) -> Vec<ColumnSpec> {
        let mut out = vec![required("id", ColumnKind::Uuid)];
        out.extend_from_slice(self.own_columns);
        out.extend_from_slice(SYNC_COLUMNS);
        if self.curated {
            out.extend_from_slice(VALIDATION_COLUMNS);
        }
        out.retain(|column| column.since <= agreed);
        out
    }

    /// The columns a push may write at `agreed`: everything the client knows that the
    /// server does not own.
    #[must_use]
    pub fn writable_at(&self, agreed: u32) -> Vec<ColumnSpec> {
        let mut out = self.columns_at(agreed);
        out.retain(|column| !column.server_owned);
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
        curated: true,
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
        curated: true,
    },
    TableSpec {
        section: "transects",
        table: "transect",
        own_columns: &[
            col("site_id", ColumnKind::Uuid),
            required("name", ColumnKind::Text),
            required("description", ColumnKind::Text),
            col("start_lat", ColumnKind::Float),
            col("start_lon", ColumnKind::Float),
            col("start_accuracy_m", ColumnKind::Float),
            col("end_lat", ColumnKind::Float),
            col("end_lon", ColumnKind::Float),
            col("end_accuracy_m", ColumnKind::Float),
            col("length_m", ColumnKind::Float),
            // Written by the `transect_depth_from_ends` trigger wherever both ends
            // are recorded, so a device's stale copy of it is not a disagreement.
            col("depth_m", ColumnKind::Float).derived(),
            col("start_depth_m", ColumnKind::Float),
            col("end_depth_m", ColumnKind::Float),
        ],
        curated: true,
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
            col("camera_label", ColumnKind::Text),
            col("rig_position", ColumnKind::Text),
            required("upside_down", ColumnKind::Bool),
            required("review", ColumnKind::Text),
            required("notes", ColumnKind::Text),
        ],
        curated: true,
    },
    TableSpec {
        section: "passes",
        table: "transect_pass",
        own_columns: &[
            col("transect_id", ColumnKind::Uuid),
            col("campaign_id", ColumnKind::Uuid),
            required("begin_s", ColumnKind::Float),
            required("end_s", ColumnKind::Float),
            col("direction", ColumnKind::Text),
            required("label", ColumnKind::Text),
            required("notes", ColumnKind::Text),
            col("quality", ColumnKind::Text),
            col("surveyed_on", ColumnKind::Date),
        ],
        curated: true,
    },
    TableSpec {
        section: "pass_videos",
        table: "pass_video",
        own_columns: &[
            required("pass_id", ColumnKind::Uuid),
            required("video_id", ColumnKind::Uuid),
            required("ordinal", ColumnKind::Int),
        ],
        curated: true,
    },
    // Pull only: presets are server-defined, and a device never authors one. Ahead of
    // runs, which name the preset they ran under.
    TableSpec {
        section: "presets",
        table: "preset",
        own_columns: &[
            required("name", ColumnKind::Text),
            required("version", ColumnKind::Int),
            required("settings", ColumnKind::Json),
            required("description", ColumnKind::Text),
        ],
        curated: false,
    },
    // Pull only, like presets and for the same reason: a profile is published by the
    // console so every laptop rectifies the same footage the same way. A device that
    // calibrates a rig publishes it through `/api/camera_calibrations/upload`, not by
    // authoring a row here.
    TableSpec {
        section: "camera_profiles",
        table: "camera_profile",
        own_columns: &[
            required("name", ColumnKind::Text),
            required("description", ColumnKind::Text),
            // Which calibration a laptop materialises for this rig. Null follows the
            // newest, so a profile no curator has deployed behaves as it always did.
            col("current_calibration_id", ColumnKind::Uuid),
        ],
        curated: false,
    },
    TableSpec {
        section: "camera_calibrations",
        table: "camera_calibration",
        own_columns: &[
            required("camera_profile_id", ColumnKind::Uuid),
            required("version", ColumnKind::Int),
            required("document", ColumnKind::Json),
            col("image_width", ColumnKind::Int),
            col("image_height", ColumnKind::Int),
            col("reprojection_error_px", ColumnKind::Float),
            col("registered_frames", ColumnKind::Int),
            required("source_clip", ColumnKind::Text),
            col("calibrated_at", ColumnKind::Timestamp),
            required("description", ColumnKind::Text),
        ],
        curated: false,
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
            col("processing_width", ColumnKind::Int),
            col("processing_height", ColumnKind::Int),
            col("fps", ColumnKind::Int),
            col("preprocess_batch_size", ColumnKind::Int),
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
            col("camera_profile", ColumnKind::Text),
            col("pixel_size_m", ColumnKind::Float),
            col("scale_type", ColumnKind::Text),
            col("transect_length_m", ColumnKind::Float),
            col("crop_width_m", ColumnKind::Float),
            col("preset_id", ColumnKind::Uuid),
            col("batch_id", ColumnKind::Uuid),
            // Which measurement of the lens the reconstruction was rectified with,
            // where the device knew: the run directory carries the document itself,
            // and this resolves it to the calibration the console holds.
            col("camera_calibration_id", ColumnKind::Uuid),
        ],
        curated: true,
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
        curated: true,
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

/// The catalogue: sections every device downloads whole.
///
/// A site, campaign or transect is defined on either side and shared by all. Presets
/// travel downwards only, since the server defines them.
pub const CLIENT_PULL_SECTIONS: &[&str] = &[
    "sites",
    "campaigns",
    "transects",
    "presets",
    "camera_profiles",
    "camera_calibrations",
];

/// Sections a device downloads restricted to the rows it authored, from contract 2, so
/// it learns what the console curated, validated or deleted.
pub const OWN_ROWS_SECTIONS: &[&str] = &["videos", "passes", "pass_videos", "runs", "cover_rows"];

/// The contract version from which [`OWN_ROWS_SECTIONS`] travel downwards.
pub const OWN_ROWS_SINCE: u32 = 1;

/// Sections a device may author rows in. The rest are read on a push, never written.
pub const CLIENT_PUSH_SECTIONS: &[&str] = &[
    "sites",
    "campaigns",
    "transects",
    "videos",
    "passes",
    "pass_videos",
    "runs",
    "cover_rows",
];

/// Whether a device pulls `section` at `agreed`.
#[must_use]
pub fn device_pulls(section: &str, agreed: u32) -> bool {
    CLIENT_PULL_SECTIONS.contains(&section)
        || (agreed >= OWN_ROWS_SINCE && OWN_ROWS_SECTIONS.contains(&section))
}

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

/// The canonical JSON form of one value, so two images of the same row compare equal:
/// numbers by kind, timestamps as `Z`, dates as `YYYY-MM-DD`. Rejects like [`to_value`].
pub fn normalise(
    spec: &ColumnSpec,
    raw: Option<&serde_json::Value>,
) -> AppResult<serde_json::Value> {
    let bound = to_value(spec, raw)?;
    Ok(match bound {
        Value::Uuid(Some(v)) => serde_json::Value::String(v.to_string()),
        Value::String(Some(v)) => serde_json::Value::String(v),
        Value::Double(Some(v)) => serde_json::Number::from_f64(v)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::Int(Some(v)) => v.into(),
        Value::BigInt(Some(v)) => v.into(),
        Value::Bool(Some(v)) => v.into(),
        Value::ChronoDateTimeUtc(Some(v)) => {
            serde_json::Value::String(v.to_rfc3339_opts(chrono::SecondsFormat::AutoSi, true))
        }
        Value::ChronoDate(Some(v)) => serde_json::Value::String(v.to_string()),
        Value::Json(Some(v)) => *v,
        _ => serde_json::Value::Null,
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
        assert_eq!(listed[listed.len() - 1], "cover_rows");
    }

    /// A section a device can neither author nor download would be unreachable to it.
    #[test]
    fn test_every_section_is_reachable_from_a_device() {
        for spec in TABLES {
            assert!(
                CLIENT_PUSH_SECTIONS.contains(&spec.section)
                    || device_pulls(spec.section, CONTRACT_VERSION),
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
                device_pulls(spec.section, CONTRACT_VERSION),
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
        assert_eq!(TABLES.len(), 11);
        for spec in TABLES {
            let names: Vec<&str> = spec.columns().iter().map(|c| c.name).collect();
            for needed in ["id", "updated_at", "deleted_at"] {
                assert!(names.contains(&needed), "{} lacks {needed}", spec.table);
            }
        }
    }

    #[test]
    fn test_a_column_added_later_is_hidden_from_an_older_client() {
        const GROWN: TableSpec = TableSpec {
            section: "grown",
            table: "grown",
            own_columns: &[
                col("first", ColumnKind::Text),
                col("later", ColumnKind::Text).since(2),
            ],
            curated: false,
        };
        let v1: Vec<&str> = GROWN.columns_at(1).iter().map(|c| c.name).collect();
        let v2: Vec<&str> = GROWN.columns_at(2).iter().map(|c| c.name).collect();
        assert!(v1.contains(&"first") && !v1.contains(&"later"));
        assert!(v2.contains(&"later"));
    }

    #[test]
    fn test_the_validation_stamp_is_read_only() {
        let spec = table_for_section("transects").expect("transects");
        let names: Vec<&str> = spec.columns().iter().map(|c| c.name).collect();
        assert!(names.contains(&"validated_at"));
        assert!(
            !spec
                .writable_at(CONTRACT_VERSION)
                .iter()
                .any(|c| c.name == "validated_at"),
            "a push may not write what the server owns"
        );
    }

    #[test]
    fn test_a_device_pulls_the_catalogue_and_its_own_upload_rows() {
        assert!(device_pulls("sites", CONTRACT_VERSION));
        assert!(device_pulls("passes", CONTRACT_VERSION));
        assert!(!device_pulls("nothing", CONTRACT_VERSION));
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
