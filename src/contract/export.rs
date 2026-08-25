//! Generation of every published contract artefact.
//!
//! Needs no database: the `OpenAPI` document comes from the router built on a
//! disconnected handle, and everything else is declared in code.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::common::AppState;
use crate::config::Config;
use crate::contract::{preset_schema, vocab};
use crate::routes::private::sync::schema::{self, CONTRACT_VERSION};

/// Where the artefacts live, relative to the repository root.
pub const CONTRACT_DIR: &str = "contract";

/// One generated file, and its path relative to the contract directory.
pub struct Artefact {
    pub path: String,
    pub body: String,
}

/// Every artefact, in the order the exporter writes them.
///
/// Four, and each answers a different question: `openapi.json` what the routes are,
/// `sync-contract.json` what a client may push, `vocabularies.json` what a coded value
/// may say and what it means, `preset-schema.json` what a preset setting accepts.
#[must_use]
pub fn artefacts() -> Vec<Artefact> {
    vec![
        Artefact {
            path: "sync-contract.json".to_string(),
            body: pretty(&sync_contract()),
        },
        Artefact {
            path: "openapi.json".to_string(),
            body: pretty(&openapi_document()),
        },
        Artefact {
            path: "vocabularies.json".to_string(),
            body: pretty(&vocabularies()),
        },
        Artefact {
            path: "preset-schema.json".to_string(),
            body: pretty(&preset_schema_document()),
        },
    ]
}

/// Write every artefact, returning the paths written.
///
/// # Errors
///
/// Returns any error from creating the directories or writing the files.
pub fn write_all(dir: &Path) -> std::io::Result<Vec<String>> {
    let mut written = Vec::new();
    for artefact in artefacts() {
        let path = dir.join(&artefact.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, &artefact.body)?;
        written.push(artefact.path);
    }
    Ok(written)
}

/// A report per stale artefact, empty when the checked-in files are current.
///
/// A file the exporter no longer produces is stale too, so orphans are reported.
#[must_use]
pub fn check(dir: &Path) -> Vec<String> {
    let artefacts = artefacts();
    let mut reports: Vec<String> = artefacts
        .iter()
        .filter_map(|artefact| {
            let on_disk = fs::read_to_string(dir.join(&artefact.path)).ok();
            report(&artefact.path, &artefact.body, on_disk.as_deref())
        })
        .collect();

    let expected: BTreeSet<&str> = artefacts.iter().map(|a| a.path.as_str()).collect();
    for orphan in generated_files_on_disk(dir) {
        if !expected.contains(orphan.as_str()) {
            reports.push(format!("{orphan}: not generated, delete it"));
        }
    }
    reports
}

/// Paths under `dir` the exporter owns. Markdown is hand-written, so it is excluded.
fn generated_files_on_disk(dir: &Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<String> = entries
        .flatten()
        .filter(|entry| entry.path().is_file())
        .map(|entry| entry.file_name().to_string_lossy().to_string())
        .filter(|name| {
            !Path::new(name)
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case("md"))
        })
        .collect();
    out.sort();
    out
}

/// How a checked-in artefact differs from the generated one, or `None` when it matches.
fn report(path: &str, expected: &str, actual: Option<&str>) -> Option<String> {
    let Some(actual) = actual else {
        return Some(format!("{path}: missing"));
    };
    if actual == expected {
        return None;
    }

    let expected_lines: Vec<&str> = expected.lines().collect();
    let actual_lines: Vec<&str> = actual.lines().collect();
    let mut lines = vec![format!(
        "{path}: stale ({} lines on disk, {} generated)",
        actual_lines.len(),
        expected_lines.len()
    )];
    let mut shown = 0;
    for i in 0..expected_lines.len().max(actual_lines.len()) {
        let old = actual_lines.get(i);
        let new = expected_lines.get(i);
        if old == new {
            continue;
        }
        if shown == DIFF_LINE_LIMIT {
            lines.push("  ... truncated".to_string());
            break;
        }
        let n = i + 1;
        lines.push(format!("  {n} - {}", old.unwrap_or(&"")));
        lines.push(format!("  {n} + {}", new.unwrap_or(&"")));
        shown += 1;
    }
    Some(lines.join("\n"))
}

/// Differing line pairs a report shows before truncating.
const DIFF_LINE_LIMIT: usize = 20;

fn pretty(value: &Value) -> String {
    let mut body = serde_json::to_string_pretty(value).expect("the artefact serialises");
    body.push('\n');
    body
}

/// The `OpenAPI` document, built on a disconnected handle so no server need be running.
fn openapi_document() -> Value {
    // The default handle is the disconnected sentinel, and nothing here queries.
    let state = AppState::new(DatabaseConnection::default(), Config::default(), None);
    let doc = crate::routes::openapi_document(&state);
    serde_json::to_value(doc).expect("the OpenAPI document serialises")
}

/// Everything a client needs to derive its own constants: the negotiable range, the
/// section names in apply order, which of them travel downwards, and the columns.
fn sync_contract() -> Value {
    json!({
        "contract_version": CONTRACT_VERSION,
        "min_contract_version": schema::MIN_CONTRACT_VERSION,
        "sections": schema::sections(),
        "pull_sections": schema::CLIENT_PULL_SECTIONS,
        "own_rows_sections": schema::OWN_ROWS_SECTIONS,
        "own_rows_since": schema::OWN_ROWS_SINCE,
        "push_sections": schema::CLIENT_PUSH_SECTIONS,
        "tables": schema::table_documents(),
    })
}

fn vocabularies() -> Value {
    json!({
        "contract_version": CONTRACT_VERSION,
        "vocabularies": vocab::VOCABULARIES,
        "legacy_exclusions": vocab::LEGACY_EXCLUSIONS,
    })
}

/// The preset field table and model catalogue the console builds its form from.
fn preset_schema_document() -> Value {
    json!({
        "preset_schema_version": preset_schema::PRESET_SCHEMA_VERSION,
        "fields": preset_schema::FIELDS,
        "choices": {
            "segmentation": preset_schema::SEGMENTATION_MODELS,
            "mapping": preset_schema::MAPPING_MODELS,
            "camera": preset_schema::CAMERA_PROFILES,
            "resolution": preset_schema::RESOLUTION_PRESETS,
        },
        "unpublishable_keys": preset_schema::UNPUBLISHABLE_KEYS,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered_lines(prefix: &str) -> String {
        use std::fmt::Write;
        let mut out = String::new();
        for i in 0..100 {
            writeln!(out, "{prefix} {i}").expect("a string accepts a write");
        }
        out
    }

    #[test]
    fn test_check_reports_stale_missing_and_orphaned_files() {
        let dir = std::env::temp_dir().join(format!("drm-contract-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        write_all(&dir).expect("the artefacts write");
        assert!(check(&dir).is_empty(), "a fresh export is current");

        fs::write(dir.join("sync-contract.json"), "{}\n").expect("the artefact is editable");
        fs::remove_file(dir.join("vocabularies.json")).expect("the file is removable");
        fs::write(dir.join("gone.json"), "{}\n").expect("the orphan writes");
        fs::write(dir.join("README.md"), "hand written\n").expect("the readme writes");

        let reports = check(&dir);
        assert!(
            reports
                .iter()
                .any(|r| r.starts_with("sync-contract.json: stale"))
        );
        assert!(reports.iter().any(|r| r == "vocabularies.json: missing"));
        assert!(
            reports
                .iter()
                .any(|r| r == "gone.json: not generated, delete it")
        );
        assert!(!reports.iter().any(|r| r.contains("README.md")));
        fs::remove_dir_all(&dir).expect("the directory clears");
    }

    #[test]
    fn test_report_matching_artefact_is_clean() {
        assert!(report("a.json", "1\n", Some("1\n")).is_none());
    }

    #[test]
    fn test_report_missing_artefact() {
        let out = report("a.json", "1\n", None).expect("a report");
        assert_eq!(out, "a.json: missing");
    }

    #[test]
    fn test_report_shows_differing_lines() {
        let out =
            report("a.json", "{\n  \"b\": 2\n}\n", Some("{\n  \"b\": 1\n}\n")).expect("a report");
        assert!(
            out.starts_with("a.json: stale (3 lines on disk, 3 generated)"),
            "{out}"
        );
        assert!(out.contains("2 -   \"b\": 1"), "{out}");
        assert!(out.contains("2 +   \"b\": 2"), "{out}");
    }

    #[test]
    fn test_report_truncates_wholesale_rewrite() {
        let expected = numbered_lines("new");
        let actual = numbered_lines("old");
        let out = report("big.json", &expected, Some(&actual)).expect("a report");
        assert!(out.ends_with("  ... truncated"), "{out}");
        assert_eq!(out.matches(" - ").count(), DIFF_LINE_LIMIT);
    }

    #[test]
    fn test_sync_contract_declares_every_table() {
        let contract = sync_contract();
        assert_eq!(contract["contract_version"], CONTRACT_VERSION);
        let tables = contract["tables"].as_array().expect("tables listed");
        assert_eq!(tables.len(), schema::TABLES.len());
    }

    #[test]
    fn test_sync_contract_carries_the_negotiable_range() {
        let contract = sync_contract();
        assert_eq!(
            contract["min_contract_version"],
            schema::MIN_CONTRACT_VERSION
        );
        let sections = contract["sections"].as_array().expect("sections listed");
        assert_eq!(sections.len(), schema::TABLES.len());
        let pull = contract["pull_sections"].as_array().expect("pull listed");
        assert!(pull.iter().all(|name| sections.contains(name)));
        let push = contract["push_sections"].as_array().expect("push listed");
        assert!(push.iter().all(|name| sections.contains(name)));
        // A section a device can neither send nor receive would be unreachable to it.
        assert!(
            sections
                .iter()
                .all(|name| pull.contains(name) || push.contains(name))
        );
    }

    #[test]
    fn test_artefacts_are_the_four_published_files() {
        let artefacts = artefacts();
        let paths: Vec<&str> = artefacts.iter().map(|a| a.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "sync-contract.json",
                "openapi.json",
                "vocabularies.json",
                "preset-schema.json"
            ]
        );
    }
}
