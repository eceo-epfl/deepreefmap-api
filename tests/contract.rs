//! Conformance of the four views of the metadata contract, against a migrated database.
//!
//! The migration spells its `CHECK` terms out literally rather than reading
//! `contract::vocab`, so these tests are what keep the two in step: a term added to a
//! vocabulary without a migration to carry it fails here.
//!
//! Every constraint is read from the live catalogue, never matched against the migration
//! source, so what the database actually has is what is asserted.

#[allow(dead_code, unused_imports)]
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use deepreefmap_api::common::AppState;
use deepreefmap_api::contract::export;
use deepreefmap_api::contract::vocab::{self, Vocabulary};
use deepreefmap_api::routes::private::sync::schema::{self, ColumnKind, TableSpec};

/// Stamped by a trigger, so it is in every syncable table and in no client-facing
/// contract.
const SERVER_STAMPED: &str = "server_seq";

/// Columns a curator owns on an otherwise device-authored table. Deliberately absent
/// from the sync contract, so a device re-pushing its row can never clobber them.
const CURATED_COLUMNS: [(&str, &str); 1] = [("transect_pass", "survey_group_id")];

/// The curated columns of one table.
fn curated(table: &str) -> impl Iterator<Item = &'static str> {
    CURATED_COLUMNS
        .iter()
        .filter(move |(t, _)| *t == table)
        .map(|(_, column)| *column)
}

/// The read-model schema each syncable table's rows appear as in the `OpenAPI`
/// components.
const RESPONSE_SCHEMAS: [(&str, &str); 9] = [
    ("site", "SiteResponse"),
    ("campaign", "CampaignResponse"),
    ("transect", "TransectResponse"),
    ("video_asset", "VideoResponse"),
    ("transect_pass", "PassResponse"),
    ("pass_video", "PassVideoResponse"),
    ("run_record", "RunResponse"),
    ("cover_row", "CoverRowResponse"),
    ("preset", "PresetResponse"),
];

/// Every list endpoint the web console reads through.
const LIST_PATHS: [&str; 11] = [
    "/sites",
    "/campaigns",
    "/transects",
    "/videos",
    "/passes",
    "/pass_groups",
    "/pass_videos",
    "/presets",
    "/runs",
    "/cover_rows",
    "/devices",
];

/// Unique indexes that must free their name once a row is tombstoned.
const PARTIAL_UNIQUE_INDEXES: [(&str, &str); 9] = [
    ("site", "site_name_lower_idx"),
    ("campaign", "campaign_name_lower_idx"),
    ("transect", "transect_site_name_lower_idx"),
    ("video_asset", "video_asset_hash_idx"),
    ("pass_video", "pass_video_ordinal_idx"),
    ("pass_video", "pass_video_unique_idx"),
    ("cover_row", "cover_row_unique_idx"),
    ("pass_group", "pass_group_name_lower_idx"),
    ("preset", "preset_name_lower_version_idx"),
];

/// Vocabulary columns the database does not enforce with a `CHECK`. Empty: every
/// vocabulary column carries one, and a new one without a migration fails here.
const UNENFORCED_VOCABULARY_COLUMNS: [&str; 0] = [];

/// Vocabulary columns whose declared nullability disagrees with the column. Empty: a
/// vocabulary that calls unrecorded a state must sit on a nullable column.
const NULLABILITY_EXCEPTIONS: [&str; 0] = [];

/// `pg_trigger.tgtype` bits, so the trigger's timing is asserted and not assumed.
const TRIGGER_ROW: i32 = 1;
const TRIGGER_BEFORE: i32 = 2;
const TRIGGER_INSERT: i32 = 4;
const TRIGGER_DELETE: i32 = 8;
const TRIGGER_UPDATE: i32 = 16;

struct DbColumn {
    data_type: String,
    nullable: bool,
}

struct CheckConstraint {
    table: String,
    name: String,
    def: String,
    columns: Vec<String>,
}

struct DbIndex {
    table: String,
    name: String,
    unique: bool,
    primary: bool,
    predicate: String,
}

struct ForeignKey {
    name: String,
    child: String,
    parent: String,
    column: String,
}

async fn rows(db: &DatabaseConnection, sql: &str) -> Vec<sea_orm::QueryResult> {
    db.query_all_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap_or_else(|e| panic!("catalogue query failed: {e}\nQuery: {sql}"))
}

fn text(row: &sea_orm::QueryResult, column: &str) -> String {
    row.try_get::<String>("", column)
        .unwrap_or_else(|e| panic!("column {column} reads as text: {e}"))
}

/// Every column of every table in the `public` schema, by table then column.
async fn database_columns(db: &DatabaseConnection) -> BTreeMap<String, BTreeMap<String, DbColumn>> {
    let sql = "SELECT table_name::text AS tbl, column_name::text AS col, \
               data_type::text AS ty, is_nullable::text AS nullable \
               FROM information_schema.columns WHERE table_schema = 'public'";
    let mut out: BTreeMap<String, BTreeMap<String, DbColumn>> = BTreeMap::new();
    for row in rows(db, sql).await {
        out.entry(text(&row, "tbl")).or_default().insert(
            text(&row, "col"),
            DbColumn {
                data_type: text(&row, "ty"),
                nullable: text(&row, "nullable") == "YES",
            },
        );
    }
    out
}

async fn check_constraints(db: &DatabaseConnection) -> Vec<CheckConstraint> {
    let sql = "SELECT c.conrelid::regclass::text AS tbl, c.conname::text AS name, \
               pg_get_constraintdef(c.oid) AS def, \
               (SELECT string_agg(a.attname::text, ',' ORDER BY a.attnum) FROM pg_attribute a \
                WHERE a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey)) AS cols \
               FROM pg_constraint c JOIN pg_class t ON t.oid = c.conrelid \
               JOIN pg_namespace n ON n.oid = t.relnamespace \
               WHERE c.contype = 'c' AND n.nspname = 'public'";
    rows(db, sql)
        .await
        .iter()
        .map(|row| CheckConstraint {
            table: text(row, "tbl"),
            name: text(row, "name"),
            def: text(row, "def"),
            columns: row
                .try_get::<Option<String>>("", "cols")
                .unwrap_or_default()
                .unwrap_or_default()
                .split(',')
                .filter(|c| !c.is_empty())
                .map(str::to_string)
                .collect(),
        })
        .collect()
}

async fn indexes(db: &DatabaseConnection) -> Vec<DbIndex> {
    let sql = "SELECT t.relname::text AS tbl, i.relname::text AS name, \
               ix.indisunique AS uniq, ix.indisprimary AS pk, \
               COALESCE(pg_get_expr(ix.indpred, ix.indrelid), '') AS predicate \
               FROM pg_index ix JOIN pg_class i ON i.oid = ix.indexrelid \
               JOIN pg_class t ON t.oid = ix.indrelid \
               JOIN pg_namespace n ON n.oid = t.relnamespace WHERE n.nspname = 'public'";
    rows(db, sql)
        .await
        .iter()
        .map(|row| DbIndex {
            table: text(row, "tbl"),
            name: text(row, "name"),
            unique: row.try_get("", "uniq").unwrap_or_default(),
            primary: row.try_get("", "pk").unwrap_or_default(),
            predicate: text(row, "predicate"),
        })
        .collect()
}

async fn foreign_keys(db: &DatabaseConnection) -> Vec<ForeignKey> {
    let sql = "SELECT c.conname::text AS name, c.conrelid::regclass::text AS child, \
               c.confrelid::regclass::text AS parent, \
               (SELECT string_agg(a.attname::text, ',' ORDER BY a.attnum) FROM pg_attribute a \
                WHERE a.attrelid = c.conrelid AND a.attnum = ANY (c.conkey)) AS cols \
               FROM pg_constraint c JOIN pg_namespace n ON n.oid = c.connamespace \
               WHERE c.contype = 'f' AND n.nspname = 'public'";
    rows(db, sql)
        .await
        .iter()
        .map(|row| ForeignKey {
            name: text(row, "name"),
            child: text(row, "child"),
            parent: text(row, "parent"),
            column: text(row, "cols"),
        })
        .collect()
}

/// The terms of a `= ANY (ARRAY[...])` list, or `None` when the constraint enumerates
/// nothing.
///
/// Postgres rewrites `IN (...)` into that form, so this is what a vocabulary `CHECK`
/// reads back as.
fn check_terms(def: &str) -> Option<Vec<String>> {
    let start = def.find("ARRAY[")? + "ARRAY[".len();
    let rest = &def[start..];
    let end = rest.find(']')?;
    Some(
        rest[..end]
            .split(',')
            .map(|term| {
                term.trim()
                    .trim_end_matches("::text")
                    .trim_matches('\'')
                    .to_string()
            })
            .collect(),
    )
}

/// Postgres data types a contract column kind may sit on.
fn acceptable_types(kind: ColumnKind) -> &'static [&'static str] {
    match kind {
        ColumnKind::Uuid => &["uuid"],
        ColumnKind::Text => &["text"],
        ColumnKind::Float => &["double precision"],
        ColumnKind::Int => &["integer"],
        ColumnKind::BigInt => &["bigint"],
        ColumnKind::Bool => &["boolean"],
        ColumnKind::Timestamp => &["timestamp with time zone"],
        ColumnKind::Date => &["date"],
        ColumnKind::Json => &["json", "jsonb"],
    }
}

fn syncable(table: &str) -> bool {
    schema::TABLES.iter().any(|spec| spec.table == table)
}

/// The vocabulary columns that sit on a syncable table, qualified.
fn vocabulary_columns() -> Vec<(&'static str, &'static Vocabulary)> {
    let mut out = Vec::new();
    for vocabulary in vocab::VOCABULARIES {
        for column in vocabulary.columns {
            let table = column.split('.').next().unwrap_or_default();
            if syncable(table) {
                out.push((*column, *vocabulary));
            }
        }
    }
    out
}

fn joined(values: &BTreeSet<&str>) -> String {
    values.iter().copied().collect::<Vec<_>>().join(", ")
}

fn contract_spec(table: &str) -> &'static TableSpec {
    schema::TABLES
        .iter()
        .find(|spec| spec.table == table)
        .unwrap_or_else(|| panic!("{table} is not a syncable table"))
}

fn assert_clean(problems: &[String], headline: &str) {
    assert!(problems.is_empty(), "{headline}\n{}", problems.join("\n"));
}

#[tokio::test]
async fn test_database_columns_match_sync_contract() {
    let db = common::setup_test_db().await;
    let database = database_columns(&db).await;
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some(actual) = database.get(spec.table) else {
            problems.push(format!(
                "{}: the sync contract names a table the database does not have",
                spec.table
            ));
            continue;
        };
        let declared: BTreeSet<&str> = spec
            .columns()
            .iter()
            .map(|c| c.name)
            .chain(std::iter::once(SERVER_STAMPED))
            .chain(curated(spec.table))
            .collect();
        let present: BTreeSet<&str> = actual.keys().map(String::as_str).collect();

        for column in present.difference(&declared) {
            problems.push(format!(
                "{}.{column}: in the database, absent from the sync contract",
                spec.table
            ));
        }
        for column in declared.difference(&present) {
            problems.push(format!(
                "{}.{column}: in the sync contract, absent from the database",
                spec.table
            ));
        }
    }

    assert_clean(
        &problems,
        "the database schema and the sync contract disagree on columns:",
    );
}

#[tokio::test]
async fn test_database_types_match_sync_contract() {
    let db = common::setup_test_db().await;
    let database = database_columns(&db).await;
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some(actual) = database.get(spec.table) else {
            continue;
        };
        for column in spec.columns() {
            let Some(found) = actual.get(column.name) else {
                continue;
            };
            let allowed = acceptable_types(column.kind);
            if !allowed.contains(&found.data_type.as_str()) {
                problems.push(format!(
                    "{}.{}: the contract declares {:?} (one of {}), the database has {}",
                    spec.table,
                    column.name,
                    column.kind,
                    allowed.join(" or "),
                    found.data_type
                ));
            }
            if found.nullable != column.nullable {
                problems.push(format!(
                    "{}.{}: the contract says {}, the database says {}",
                    spec.table,
                    column.name,
                    if column.nullable {
                        "nullable"
                    } else {
                        "required"
                    },
                    if found.nullable { "NULL" } else { "NOT NULL" }
                ));
            }
        }
    }

    assert_clean(
        &problems,
        "the database schema and the sync contract disagree on types:",
    );
}

#[tokio::test]
async fn test_check_constraints_match_vocabularies() {
    let db = common::setup_test_db().await;
    let mut problems = Vec::new();

    for constraint in check_constraints(&db).await {
        if !syncable(&constraint.table) {
            continue;
        }
        let Some(terms) = check_terms(&constraint.def) else {
            continue;
        };
        let claimed: Vec<(String, &Vocabulary)> = constraint
            .columns
            .iter()
            .filter_map(|column| {
                let qualified = format!("{}.{column}", constraint.table);
                vocab::vocabulary_for_column(&qualified).map(|v| (qualified, v))
            })
            .collect();

        let [(qualified, vocabulary)] = claimed.as_slice() else {
            problems.push(format!(
                "{}: CHECK {} enumerates {} over column(s) {} but {} vocabularies claim them",
                constraint.table,
                constraint.name,
                terms.join(", "),
                constraint.columns.join(", "),
                claimed.len()
            ));
            continue;
        };

        let listed: BTreeSet<&str> = terms.iter().map(String::as_str).collect();
        let codes: BTreeSet<&str> = vocabulary.codes().into_iter().collect();
        if listed == codes && listed.len() == terms.len() {
            continue;
        }
        problems.push(format!(
            "{qualified}: vocabulary {} declares [{}], CHECK {} allows [{}] \
             (only in the vocabulary: [{}]; only in the constraint: [{}])",
            vocabulary.name,
            joined(&codes),
            constraint.name,
            joined(&listed),
            joined(&codes.difference(&listed).copied().collect()),
            joined(&listed.difference(&codes).copied().collect()),
        ));
    }

    assert_clean(
        &problems,
        "a CHECK constraint and its vocabulary disagree, so add a migration or a term:",
    );
}

#[tokio::test]
async fn test_every_vocabulary_column_has_check_constraint() {
    let db = common::setup_test_db().await;
    let constraints = check_constraints(&db).await;
    let mut problems = Vec::new();

    for (qualified, vocabulary) in vocabulary_columns() {
        let (table, column) = qualified.split_once('.').expect("a qualified column");
        let enforcing: Vec<&str> = constraints
            .iter()
            .filter(|c| {
                c.table == table
                    && c.columns.iter().any(|name| name == column)
                    && check_terms(&c.def).is_some()
            })
            .map(|c| c.name.as_str())
            .collect();
        let excepted = UNENFORCED_VOCABULARY_COLUMNS.contains(&qualified);

        match (enforcing.len(), excepted) {
            (1, false) | (0, true) => {}
            (0, false) => problems.push(format!(
                "{qualified}: vocabulary {} constrains it and no CHECK enforces it, \
                 so any string is accepted",
                vocabulary.name
            )),
            (_, true) => problems.push(format!(
                "{qualified}: now enforced by {}, so drop it from \
                 UNENFORCED_VOCABULARY_COLUMNS",
                enforcing.join(", ")
            )),
            (_, false) => problems.push(format!(
                "{qualified}: {} CHECK constraints enumerate it ({})",
                enforcing.len(),
                enforcing.join(", ")
            )),
        }
    }

    assert_clean(
        &problems,
        "a vocabulary column is unenforced or doubly enforced:",
    );
}

#[tokio::test]
async fn test_every_vocabulary_column_exists_in_database() {
    let db = common::setup_test_db().await;
    let database = database_columns(&db).await;
    let mut problems = Vec::new();

    for vocabulary in vocab::VOCABULARIES {
        for qualified in vocabulary.columns {
            let (table, column) = qualified.split_once('.').expect("a qualified column");
            let Some(columns) = database.get(table) else {
                problems.push(format!(
                    "{qualified}: vocabulary {} names a table the database does not have",
                    vocabulary.name
                ));
                continue;
            };
            if !columns.contains_key(column) {
                problems.push(format!(
                    "{qualified}: vocabulary {} names a column {table} does not have",
                    vocabulary.name
                ));
            }
        }
    }

    assert_clean(
        &problems,
        "a vocabulary constrains a column that does not exist:",
    );
}

#[tokio::test]
async fn test_vocabulary_nullability_matches_column() {
    let db = common::setup_test_db().await;
    let database = database_columns(&db).await;
    let constraints = check_constraints(&db).await;
    let mut problems = Vec::new();

    for (qualified, vocabulary) in vocabulary_columns() {
        let (table, column) = qualified.split_once('.').expect("a qualified column");
        if NULLABILITY_EXCEPTIONS.contains(&qualified) {
            continue;
        }
        let Some(found) = database.get(table).and_then(|cols| cols.get(column)) else {
            continue;
        };
        if found.nullable != vocabulary.nullable {
            problems.push(format!(
                "{qualified}: vocabulary {} declares nullable={}, the database says {}",
                vocabulary.name,
                vocabulary.nullable,
                if found.nullable { "NULL" } else { "NOT NULL" }
            ));
        }
        for constraint in constraints.iter().filter(|c| {
            c.table == table
                && c.columns.iter().any(|name| name == column)
                && check_terms(&c.def).is_some()
        }) {
            let admits_null = constraint.def.contains(&format!("{column} IS NULL"));
            if admits_null != vocabulary.nullable {
                problems.push(format!(
                    "{qualified}: vocabulary {} declares nullable={}, CHECK {} {} null",
                    vocabulary.name,
                    vocabulary.nullable,
                    constraint.name,
                    if admits_null { "admits" } else { "refuses" }
                ));
            }
        }
    }

    assert_clean(
        &problems,
        "a vocabulary and its column disagree on whether null is a state:",
    );
}

#[tokio::test]
async fn test_every_syncable_table_carries_sync_columns() {
    let db = common::setup_test_db().await;
    let database = database_columns(&db).await;
    let required = [
        "created_at",
        "updated_at",
        "deleted_at",
        "device_id",
        SERVER_STAMPED,
    ];
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some(actual) = database.get(spec.table) else {
            continue;
        };
        for column in required {
            if !actual.contains_key(column) {
                problems.push(format!(
                    "{}.{column}: a syncable table without it cannot replicate",
                    spec.table
                ));
            }
        }
    }

    assert_clean(&problems, "a syncable table is missing a sync column:");
}

#[tokio::test]
async fn test_server_seq_never_client_writable() {
    let db = common::setup_test_db().await;
    let state = AppState::new(db, common::test_config(), None);
    let doc = serde_json::to_value(deepreefmap_api::routes::openapi_document(&state))
        .expect("the OpenAPI document serialises");
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        if spec.columns().iter().any(|c| c.name == SERVER_STAMPED) {
            problems.push(format!(
                "{}: the sync contract accepts {SERVER_STAMPED} from a client",
                spec.table
            ));
        }
    }

    let schemas = doc
        .pointer("/components/schemas")
        .and_then(serde_json::Value::as_object)
        .expect("the document declares component schemas");
    for (name, schema) in schemas {
        if !name.ends_with("Create") && !name.ends_with("Update") {
            continue;
        }
        if schema
            .pointer("/properties")
            .and_then(serde_json::Value::as_object)
            .is_some_and(|props| props.contains_key(SERVER_STAMPED))
        {
            problems.push(format!("{name}: exposes {SERVER_STAMPED} as writable"));
        }
    }

    assert_clean(
        &problems,
        "a client can set its own position in someone else's pull order:",
    );
}

#[tokio::test]
async fn test_server_seq_trigger_on_every_syncable_table() {
    let db = common::setup_test_db().await;
    let sql = "SELECT t.tgrelid::regclass::text AS tbl, t.tgname::text AS name, \
               t.tgtype::int AS tgtype, t.tgenabled::text AS enabled \
               FROM pg_trigger t JOIN pg_proc p ON p.oid = t.tgfoid \
               WHERE NOT t.tgisinternal AND p.proname = 'set_server_seq'";
    let attached: BTreeMap<String, (i32, String)> = rows(&db, sql)
        .await
        .iter()
        .map(|row| {
            (
                text(row, "tbl"),
                (
                    row.try_get::<i32>("", "tgtype").unwrap_or_default(),
                    text(row, "enabled"),
                ),
            )
        })
        .collect();
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some((tgtype, enabled)) = attached.get(spec.table) else {
            problems.push(format!(
                "{}: no trigger runs set_server_seq, so its rows are invisible to pulls",
                spec.table
            ));
            continue;
        };
        if enabled != "O" {
            problems.push(format!(
                "{}: the set_server_seq trigger is disabled (tgenabled {enabled})",
                spec.table
            ));
        }
        for (bit, label) in [
            (TRIGGER_ROW, "FOR EACH ROW"),
            (TRIGGER_BEFORE, "BEFORE"),
            (TRIGGER_INSERT, "INSERT"),
            (TRIGGER_UPDATE, "UPDATE"),
        ] {
            if tgtype & bit == 0 {
                problems.push(format!(
                    "{}: the set_server_seq trigger is not {label} (tgtype {tgtype})",
                    spec.table
                ));
            }
        }
        if tgtype & TRIGGER_DELETE != 0 {
            problems.push(format!(
                "{}: the set_server_seq trigger fires on DELETE (tgtype {tgtype})",
                spec.table
            ));
        }
    }

    assert_clean(&problems, "the server_seq trigger is not attached:");
}

#[tokio::test]
async fn test_unique_indexes_partial_on_deleted_at() {
    let db = common::setup_test_db().await;
    let all = indexes(&db).await;
    let mut problems = Vec::new();

    for (table, name) in PARTIAL_UNIQUE_INDEXES {
        let Some(index) = all.iter().find(|i| i.table == table && i.name == name) else {
            problems.push(format!(
                "{table}: index {name} does not exist, so a tombstone keeps holding its name"
            ));
            continue;
        };
        if !index.unique {
            problems.push(format!("{table}.{name}: exists but is not unique"));
        }
        if !index.predicate.contains("deleted_at IS NULL") {
            problems.push(format!(
                "{table}.{name}: not partial on deleted_at IS NULL (predicate {:?})",
                index.predicate
            ));
        }
    }

    for index in all
        .iter()
        .filter(|i| i.unique && !i.primary && syncable(&i.table))
    {
        if !index.predicate.contains("deleted_at IS NULL") {
            problems.push(format!(
                "{}.{}: unique over tombstones too (predicate {:?}), so a delete cannot free the value",
                index.table, index.name, index.predicate
            ));
        }
    }

    assert_clean(&problems, "tombstoning does not free a unique value:");
}

#[tokio::test]
async fn test_tables_in_foreign_key_order() {
    let db = common::setup_test_db().await;
    let order: BTreeMap<&str, usize> = schema::TABLES
        .iter()
        .enumerate()
        .map(|(i, spec)| (spec.table, i))
        .collect();
    let mut problems = Vec::new();

    for key in foreign_keys(&db).await {
        if !syncable(&key.child) || !syncable(&key.parent) || key.child == key.parent {
            continue;
        }
        let child = order[key.child.as_str()];
        let parent = order[key.parent.as_str()];
        if parent > child {
            problems.push(format!(
                "{}.{} references {} ({}), which TABLES lists at position {parent}, \
                 after {} at position {child}",
                key.child, key.column, key.parent, key.name, key.child
            ));
        }
    }

    assert_clean(
        &problems,
        "TABLES is not in foreign-key order, so a push cannot apply top to bottom:",
    );
}

#[tokio::test]
async fn test_entity_schemas_expose_contract_columns() {
    let db = common::setup_test_db().await;
    let state = AppState::new(db, common::test_config(), None);
    let doc = serde_json::to_value(deepreefmap_api::routes::openapi_document(&state))
        .expect("the OpenAPI document serialises");
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some((_, name)) = RESPONSE_SCHEMAS.iter().find(|(t, _)| *t == spec.table) else {
            problems.push(format!("{}: no response schema is declared", spec.table));
            continue;
        };
        let Some(properties) = doc
            .pointer(&format!("/components/schemas/{name}/properties"))
            .and_then(serde_json::Value::as_object)
        else {
            problems.push(format!(
                "{}: the OpenAPI document has no object schema {name}",
                spec.table
            ));
            continue;
        };

        let declared: BTreeSet<&str> = spec
            .columns()
            .iter()
            .map(|c| c.name)
            .chain(std::iter::once(SERVER_STAMPED))
            .chain(curated(spec.table))
            .collect();
        let exposed: BTreeSet<&str> = properties.keys().map(String::as_str).collect();

        for field in exposed.difference(&declared) {
            problems.push(format!(
                "{name}.{field}: exposed by the entity, absent from the {} contract",
                spec.table
            ));
        }
        for field in declared.difference(&exposed) {
            problems.push(format!(
                "{name}.{field}: in the {} contract, not exposed by the entity",
                spec.table
            ));
        }
    }

    assert_clean(
        &problems,
        "an entity and its contract table expose different fields:",
    );
}

#[tokio::test]
async fn test_list_endpoints_filter_on_id() {
    let db = common::setup_test_db().await;
    let state = AppState::new(db, common::test_config(), None);
    let doc = serde_json::to_value(deepreefmap_api::routes::openapi_document(&state))
        .expect("the OpenAPI document serialises");
    let mut problems = Vec::new();

    for path in LIST_PATHS {
        // A JSON pointer token escapes the slashes inside a path key.
        let pointer = format!("/paths/{}/get/description", path.replace('/', "~1"));
        let Some(description) = doc.pointer(&pointer).and_then(serde_json::Value::as_str) else {
            problems.push(format!(
                "{path}: no GET description in the OpenAPI document"
            ));
            continue;
        };
        let Some((_, filterable)) = description.split_once("Additional filterable columns:") else {
            problems.push(format!(
                "{path}: the GET description lists no filterable columns"
            ));
            continue;
        };
        // The last column in the list carries the sentence's full stop.
        if !filterable
            .lines()
            .any(|line| line.trim().trim_end_matches('.') == "- id")
        {
            problems.push(format!("{path}: id is not filterable"));
        }
    }

    assert_clean(&problems, "a list endpoint cannot be filtered by id:");
}

#[test]
fn test_published_artefacts_current() {
    let stale = export::check(&contract_dir());
    assert!(
        stale.is_empty(),
        "{} contract artefact(s) are stale, run `cargo run --bin export-contract`:\n{}",
        stale.len(),
        stale.join("\n")
    );
}

#[test]
fn test_published_sync_contract_covers_every_table() {
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(contract_dir().join("sync-contract.json"))
            .expect("the published sync contract reads"),
    )
    .expect("the published sync contract parses");
    let tables = published["tables"].as_array().expect("tables");
    let mut problems = Vec::new();

    for spec in schema::TABLES {
        let Some(entry) = tables
            .iter()
            .find(|t| t["table"].as_str() == Some(spec.table))
        else {
            problems.push(format!("{}: absent from sync-contract.json", spec.table));
            continue;
        };
        let published: BTreeSet<&str> = entry["columns"]
            .as_array()
            .expect("columns")
            .iter()
            .filter_map(|c| c["name"].as_str())
            .collect();
        let declared: BTreeSet<&str> = contract_spec(spec.table)
            .columns()
            .iter()
            .map(|c| c.name)
            .collect();
        for column in declared.symmetric_difference(&published) {
            problems.push(format!(
                "{}.{column}: sync-contract.json and TABLES disagree on it",
                spec.table
            ));
        }
    }

    assert_clean(
        &problems,
        "the published sync contract does not describe the live one:",
    );
}

/// The Python client derives its own constants from this artefact, so the negotiable range
/// and both section lists have to be in it.
#[test]
fn test_published_sync_contract_carries_the_negotiable_range() {
    let published: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(contract_dir().join("sync-contract.json"))
            .expect("the published sync contract reads"),
    )
    .expect("the published sync contract parses");

    assert_eq!(
        published["min_contract_version"],
        schema::MIN_CONTRACT_VERSION
    );
    assert_eq!(published["contract_version"], schema::CONTRACT_VERSION);

    let sections: BTreeSet<&str> = published["sections"]
        .as_array()
        .expect("sections listed")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    let declared: BTreeSet<&str> = schema::sections().into_iter().collect();
    assert_eq!(sections, declared);

    let pull: BTreeSet<&str> = published["pull_sections"]
        .as_array()
        .expect("pull_sections listed")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    assert!(
        pull.is_subset(&sections),
        "pull_sections names something that is not a section: {pull:?}"
    );
}

fn contract_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(export::CONTRACT_DIR)
}
