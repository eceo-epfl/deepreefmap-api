//! Pins the list and read surface the console depends on: response key sets,
//! `Content-Range` totals, pagination, sorting and filter handling. A change in the
//! CRUD machinery shows up as a diff here, not in the console.

#[allow(dead_code)]
mod common;

use common::*;

fn site_body(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "description": "" })
}

async fn create_site(app: &axum::Router, name: &str) -> String {
    let (status, body) = post_json(app, "/api/sites", &site_body(name), None).await;
    assert_eq!(status, 201, "site create failed: {body}");
    body["id"].as_str().expect("id in response").to_string()
}

/// The sorted key set of one JSON object, so a shape assertion reads as one line.
fn keys(value: &serde_json::Value) -> Vec<String> {
    let mut keys: Vec<String> = value
        .as_object()
        .unwrap_or_else(|| panic!("expected an object: {value}"))
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn content_range(headers: &axum::http::HeaderMap) -> String {
    headers
        .get("Content-Range")
        .expect("Content-Range header")
        .to_str()
        .expect("Content-Range is ASCII")
        .to_string()
}

/// A percent-encoded query string, so filter and sort JSON survives the URI.
fn query(pairs: &[(&str, &str)]) -> String {
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    for (key, value) in pairs {
        serializer.append_pair(key, value);
    }
    serializer.finish()
}

fn uuid(tag: &str) -> String {
    format!("{tag:0>8}-0000-4000-8000-000000000000")
}

fn pass_row(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "begin_s": 0.0,
        "end_s": 120.0,
        "upside_down": false,
        "label": "",
        "notes": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn run_row(id: &str, pass_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "pass_id": pass_id,
        "status": "succeeded",
        "started_at": "2026-08-01T09:00:00Z",
        "error": "",
        "run_dir_name": id,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

const SITE_KEYS: &[&str] = &[
    "country",
    "created_at",
    "deleted_at",
    "description",
    "device_id",
    "id",
    "latitude",
    "longitude",
    "name",
    "region",
    "server_seq",
    "updated_at",
];

const PRESET_ONE_KEYS: &[&str] = &[
    "created_at",
    "deleted_at",
    "description",
    "device_id",
    "id",
    "name",
    "server_seq",
    "settings",
    "updated_at",
    "version",
];

/// The list drops `settings`: a page of presets does not carry every document.
const PRESET_LIST_KEYS: &[&str] = &[
    "created_at",
    "deleted_at",
    "description",
    "device_id",
    "id",
    "name",
    "server_seq",
    "updated_at",
    "version",
];

const RUN_ONE_KEYS: &[&str] = &[
    "created_at",
    "deleted_at",
    "device_id",
    "error",
    "finished_at",
    "fps",
    "gui_version",
    "id",
    "library_version",
    "mapping_backend",
    "model_revisions",
    "pass_id",
    "preprocess_batch_size",
    "preset_deviations",
    "preset_hash",
    "preset_name",
    "preset_version",
    "processing_height",
    "processing_width",
    "run_dir_name",
    "run_duration_s",
    "segmentation_model",
    "server_seq",
    "stage_durations",
    "stage_peaks",
    "started_at",
    "status",
    "taxonomy_hash",
    "taxonomy_version",
    "updated_at",
];

/// The list drops the heavy provenance blobs. `preset_deviations` stays: the run list
/// renders its Deviations chip from list rows.
const RUN_LIST_KEYS: &[&str] = &[
    "created_at",
    "deleted_at",
    "device_id",
    "error",
    "finished_at",
    "fps",
    "gui_version",
    "id",
    "library_version",
    "mapping_backend",
    "pass_id",
    "preprocess_batch_size",
    "preset_deviations",
    "preset_hash",
    "preset_name",
    "preset_version",
    "processing_height",
    "processing_width",
    "run_dir_name",
    "run_duration_s",
    "segmentation_model",
    "server_seq",
    "started_at",
    "status",
    "taxonomy_hash",
    "taxonomy_version",
    "updated_at",
];

/// Every device column except the token pair, which no read path may return.
const DEVICE_ONE_KEYS: &[&str] = &[
    "active_preset_name",
    "active_preset_reported_at",
    "active_preset_version",
    "assigned_at",
    "assigned_preset_id",
    "created_at",
    "enrolled_by",
    "gui_version",
    "id",
    "last_seen_at",
    "library_version",
    "name",
    "platform",
    "preset_schema_version",
    "profile_reported_at",
    "revoked_at",
    "system_profile",
    "versions_changed_at",
];

/// The list also drops `system_profile`.
const DEVICE_LIST_KEYS: &[&str] = &[
    "active_preset_name",
    "active_preset_reported_at",
    "active_preset_version",
    "assigned_at",
    "assigned_preset_id",
    "created_at",
    "enrolled_by",
    "gui_version",
    "id",
    "last_seen_at",
    "library_version",
    "name",
    "platform",
    "preset_schema_version",
    "profile_reported_at",
    "revoked_at",
    "versions_changed_at",
];

#[tokio::test]
async fn test_site_list_and_one_key_sets() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;

    let (status, listed) = get_json(&app, "/api/sites", None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&listed[0]), SITE_KEYS, "list element shape drifted");

    let (status, one) = get_json(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&one), SITE_KEYS, "get-one shape drifted");
}

#[tokio::test]
async fn test_preset_list_and_one_key_sets() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, created) = post_json(
        &app,
        "/api/presets",
        &serde_json::json!({
            "name": "eceo-default",
            "version": 1,
            "settings": { "fps": 4 },
            "description": "",
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().expect("id").to_string();

    let (status, listed) = get_json(&app, "/api/presets", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        keys(&listed[0]),
        PRESET_LIST_KEYS,
        "list element shape drifted"
    );

    let (status, one) = get_json(&app, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&one), PRESET_ONE_KEYS, "get-one shape drifted");
}

#[tokio::test]
async fn test_run_list_and_one_key_sets() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let pass = uuid("b1");
    let run = uuid("c1");
    let document = push_body(&serde_json::json!({
        "passes": [pass_row(&pass)],
        "runs": [run_row(&run, &pass)],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(&admin, "/api/runs", None).await;
    assert_eq!(status, 200);
    assert_eq!(
        keys(&listed[0]),
        RUN_LIST_KEYS,
        "list element shape drifted"
    );

    let (status, one) = get_json(&admin, &format!("/api/runs/{run}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&one), RUN_ONE_KEYS, "get-one shape drifted");
}

#[tokio::test]
async fn test_device_list_and_one_key_sets() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    enrol_device(&app, &code).await;

    let (status, listed) = get_json(&admin, "/api/devices", None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&listed[0]), DEVICE_LIST_KEYS, "a token column leaked");
    let id = listed[0]["id"].as_str().expect("id").to_string();

    let (status, one) = get_json(&admin, &format!("/api/devices/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(keys(&one), DEVICE_ONE_KEYS, "a token column leaked");
}

/// The page and its total must agree on what a tombstone is, or the console paginates
/// into phantom rows.
#[tokio::test]
async fn test_list_content_range_excludes_tombstones() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let doomed = create_site(&app, "Harat").await;
    create_site(&app, "Fanous").await;
    create_site(&app, "Marsa").await;
    let (status, _) = delete(&app, &format!("/api/sites/{doomed}"), None).await;
    assert_eq!(status, 204);

    let (status, headers, listed) = get_json_with_headers(&app, "/api/sites", None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().expect("array").len(), 2);
    assert_eq!(
        content_range(&headers),
        "sites 0-1/2",
        "the total disagrees with the page"
    );
}

#[tokio::test]
async fn test_list_content_range_paginates() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    for name in ["a", "b", "c", "d", "e"] {
        create_site(&app, name).await;
    }

    let uri = format!("/api/sites?{}", query(&[("range", "[0,1]")]));
    let (status, headers, listed) = get_json_with_headers(&app, &uri, None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().expect("array").len(), 2);
    assert_eq!(content_range(&headers), "sites 0-1/5");

    let uri = format!("/api/sites?{}", query(&[("range", "[4,9]")]));
    let (status, headers, listed) = get_json_with_headers(&app, &uri, None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().expect("array").len(), 1);
    assert_eq!(content_range(&headers), "sites 4-4/5");
}

/// `per_page=0` clamps to one row rather than serving a page that never advances.
#[tokio::test]
async fn test_list_per_page_zero_clamps() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    create_site(&app, "Harat").await;
    create_site(&app, "Fanous").await;

    let uri = format!("/api/sites?{}", query(&[("page", "1"), ("per_page", "0")]));
    let (status, headers, listed) = get_json_with_headers(&app, &uri, None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().expect("array").len(), 1);
    assert_eq!(content_range(&headers), "sites 0-0/2");
}

#[tokio::test]
async fn test_list_out_of_range_page_is_empty_not_an_error() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    create_site(&app, "Harat").await;

    let uri = format!("/api/sites?{}", query(&[("range", "[100,110]")]));
    let (status, headers, listed) = get_json_with_headers(&app, &uri, None).await;
    assert_eq!(status, 200);
    assert!(listed.as_array().expect("array").is_empty());
    assert!(
        content_range(&headers).ends_with("/1"),
        "the total forgot the rows before the horizon: {}",
        content_range(&headers)
    );
}

#[tokio::test]
async fn test_list_sorts_both_ways() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    for name in ["b", "c", "a"] {
        create_site(&app, name).await;
    }

    for (order, expected) in [("ASC", ["a", "b", "c"]), ("DESC", ["c", "b", "a"])] {
        let sort = format!("[\"name\",\"{order}\"]");
        let uri = format!("/api/sites?{}", query(&[("sort", &sort)]));
        let (status, listed) = get_json(&app, &uri, None).await;
        assert_eq!(status, 200);
        let names: Vec<&str> = listed
            .as_array()
            .expect("array")
            .iter()
            .map(|s| s["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, expected, "sort {order} broke");
    }
}

/// Rows tied on the sort column must not repeat or vanish across a page boundary.
#[tokio::test]
async fn test_list_sort_ties_page_stably() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    // Country ties both rows; the primary key must break the tie the same way twice.
    let mut ids = vec![];
    for name in ["Harat", "Fanous"] {
        let (status, body) = post_json(
            &app,
            "/api/sites",
            &serde_json::json!({ "name": name, "description": "", "country": "Egypt" }),
            None,
        )
        .await;
        assert_eq!(status, 201, "{body}");
        ids.push(body["id"].as_str().expect("id").to_string());
    }

    let mut seen = vec![];
    for page in ["1", "2"] {
        let uri = format!(
            "/api/sites?{}",
            query(&[
                ("sort", "[\"country\",\"ASC\"]"),
                ("page", page),
                ("per_page", "1"),
            ])
        );
        let (status, listed) = get_json(&app, &uri, None).await;
        assert_eq!(status, 200);
        let page_rows = listed.as_array().expect("array");
        assert_eq!(page_rows.len(), 1);
        seen.push(page_rows[0]["id"].as_str().expect("id").to_string());
    }
    seen.sort();
    ids.sort();
    assert_eq!(seen, ids, "a tied row repeated or vanished across pages");
}

#[tokio::test]
async fn test_list_sort_by_unsortable_column_falls_back() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    create_site(&app, "Harat").await;

    // `description` is not sortable, so the default order applies rather than a 500.
    let uri = format!(
        "/api/sites?{}",
        query(&[("sort", "[\"description\",\"ASC\"]")])
    );
    let (status, listed) = get_json(&app, &uri, None).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed.as_array().expect("array").len(), 1);
}

/// react-admin's `getMany` fetches references as `filter={"id":[...]}`.
#[tokio::test]
async fn test_list_filter_by_id_array() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let first = create_site(&app, "Harat").await;
    let second = create_site(&app, "Fanous").await;
    create_site(&app, "Marsa").await;

    let filter = format!("{{\"id\":[\"{first}\",\"{second}\"]}}");
    let uri = format!("/api/sites?{}", query(&[("filter", &filter)]));
    let (status, listed) = get_json(&app, &uri, None).await;
    assert_eq!(status, 200);
    let mut got: Vec<&str> = listed
        .as_array()
        .expect("array")
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    got.sort_unstable();
    let mut expected = [first.as_str(), second.as_str()];
    expected.sort_unstable();
    assert_eq!(got, expected);
}

#[tokio::test]
async fn test_list_filter_unknown_key_is_ignored() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    create_site(&app, "Harat").await;
    create_site(&app, "Fanous").await;

    let uri = format!(
        "/api/sites?{}",
        query(&[("filter", "{\"no_such_column\":\"x\"}")])
    );
    let (status, listed) = get_json(&app, &uri, None).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed.as_array().expect("array").len(), 2);
}

/// The tombstone guard rebuilds the filter, so an unparseable one is dropped rather
/// than reaching the strict parser as a 400.
#[tokio::test]
async fn test_list_filter_malformed_json_is_ignored() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    create_site(&app, "Harat").await;

    let uri = format!("/api/sites?{}", query(&[("filter", "{not json")]));
    let (status, listed) = get_json(&app, &uri, None).await;
    assert_eq!(status, 200, "{listed}");
    assert_eq!(listed.as_array().expect("array").len(), 1);
}

/// Asking for tombstones must not get them, and the total must agree.
#[tokio::test]
async fn test_list_caller_deleted_at_filter_keeps_live_rows() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let doomed = create_site(&app, "Harat").await;
    create_site(&app, "Fanous").await;
    let (status, _) = delete(&app, &format!("/api/sites/{doomed}"), None).await;
    assert_eq!(status, 204);

    let uri = format!(
        "/api/sites?{}",
        query(&[("filter", "{\"deleted_at\":\"not_null\"}")])
    );
    let (status, headers, listed) = get_json_with_headers(&app, &uri, None).await;
    assert_eq!(status, 200);
    let rows = listed.as_array().expect("array");
    assert_eq!(rows.len(), 1, "the guard lost to a caller filter: {listed}");
    assert_eq!(rows[0]["name"], "Fanous");
    assert_eq!(content_range(&headers), "sites 0-0/1");
}

#[tokio::test]
async fn test_get_one_preset_hides_a_tombstone() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, created) = post_json(
        &app,
        "/api/presets",
        &serde_json::json!({
            "name": "eceo-default",
            "version": 1,
            "settings": {},
            "description": "",
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().expect("id").to_string();

    let (status, _) = delete(&app, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 204);

    let (status, body) = get(&app, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 404, "get-one served a tombstone: {body}");
}

/// Trimming a field from the list must not touch what is stored or what get-one returns.
#[tokio::test]
async fn test_preset_settings_survive_the_list_trim() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let settings =
        serde_json::json!({ "fps": 4, "mapping_backend": "loger", "nested": { "a": [1, 2] } });
    let (status, created) = post_json(
        &app,
        "/api/presets",
        &serde_json::json!({
            "name": "eceo-default",
            "version": 1,
            "settings": settings,
            "description": "",
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().expect("id").to_string();

    let (status, one) = get_json(&app, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(one["settings"], settings, "get-one lost the document");

    let replacement = serde_json::json!({ "fps": 8 });
    let (status, body) = put(
        &app,
        &format!("/api/presets/{id}"),
        &serde_json::json!({ "settings": replacement }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let stored: serde_json::Value = one_value(
        &db,
        &format!("SELECT settings FROM preset WHERE id = '{id}'"),
    )
    .await;
    assert_eq!(stored, replacement, "the update did not land whole");
}

#[tokio::test]
async fn test_device_system_profile_survives_the_list_trim() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let profile = serde_json::json!({ "gpu": "RTX 4070 Laptop", "ram_gb": 32 });
    let (status, body) = post(
        &app,
        "/api/sync/heartbeat",
        &serde_json::json!({ "system_profile": profile }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(&admin, "/api/devices", None).await;
    assert_eq!(status, 200);
    assert!(
        listed[0].get("system_profile").is_none(),
        "the list still carries the profile: {listed}"
    );

    let id = listed[0]["id"].as_str().expect("id").to_string();
    let (status, one) = get_json(&admin, &format!("/api/devices/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(one["system_profile"], profile, "get-one lost the profile");
}

#[tokio::test]
async fn test_run_provenance_survives_the_list_trim() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let revisions = serde_json::json!({ "deepreefmap": "abc123" });
    let durations = serde_json::json!({ "mapping": 41.5 });
    let peaks = serde_json::json!({ "mapping": { "vram_gb": 7.9 } });

    let pass = uuid("b1");
    let run = uuid("c1");
    let mut row = run_row(&run, &pass);
    row["model_revisions"] = revisions.clone();
    row["stage_durations"] = durations.clone();
    row["stage_peaks"] = peaks.clone();
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "passes": [pass_row(&pass)], "runs": [row] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(&admin, "/api/runs", None).await;
    assert_eq!(status, 200);
    for field in ["model_revisions", "stage_durations", "stage_peaks"] {
        assert!(
            listed[0].get(field).is_none(),
            "the list still carries {field}: {listed}"
        );
    }

    let (status, one) = get_json(&admin, &format!("/api/runs/{run}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(one["model_revisions"], revisions);
    assert_eq!(one["stage_durations"], durations);
    assert_eq!(one["stage_peaks"], peaks);
}

#[tokio::test]
async fn test_get_one_run_hides_a_tombstone() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let pass = uuid("b1");
    let run = uuid("c1");
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "passes": [pass_row(&pass)],
            "runs": [run_row(&run, &pass)],
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let mut tombstoned = run_row(&run, &pass);
    tombstoned["updated_at"] = serde_json::json!("2026-08-01T11:00:00Z");
    tombstoned["deleted_at"] = serde_json::json!("2026-08-01T11:00:00Z");
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "runs": [tombstoned] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = get(&admin, &format!("/api/runs/{run}"), None).await;
    assert_eq!(status, 404, "get-one served a tombstone: {body}");
}
