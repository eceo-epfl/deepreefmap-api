//! Sync round-trip behaviour: what a laptop pushes, what another laptop pulls, and
//! what happens when both edited the same row.

// Digit separators inside a latitude or longitude make it unreadable as a coordinate.
#![allow(clippy::unreadable_literal)]

#[allow(dead_code)]
mod common;

use common::*;

/// Two devices enrolled by two people.
async fn two_devices(app: &axum::Router, db: &sea_orm::DatabaseConnection) -> (String, String) {
    let code_a = seed_connect_code(db, "alice", "Alice laptop").await;
    let code_b = seed_connect_code(db, "bob", "Bob laptop").await;
    let token_a = enrol_device(app, &code_a).await;
    let token_b = enrol_device(app, &code_b).await;
    (token_a, token_b)
}

fn site_row(id: &str, name: &str, updated_at: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": updated_at,
    })
}

/// A transect as a laptop sends it: the shallowest section a device actually authors.
fn transect_row(id: &str, name: &str, updated_at: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "start_lat": 16.202559, "start_lon": 39.443041,
        "end_lat": 16.20168, "end_lon": 39.442997,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": updated_at,
    })
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

#[tokio::test]
async fn test_push_then_pull_on_another_device() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let (token_a, token_b) = two_devices(&app, &db).await;

    let transect_id = "11111111-1111-4111-8111-111111111111";
    let (status, pushed) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(transect_id, "Japanese Garden", "2026-08-01T10:00:00Z")]
        })),
        Some(&token_a),
    )
    .await;
    assert_eq!(status, 200, "push failed: {pushed}");
    assert_eq!(pushed["sections"]["transects"]["applied"], 1);
    assert!(
        pushed["sections"]["transects"]["skipped"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let (status, pulled) = get_json(&app, "/api/sync/pull", Some(&token_b)).await;
    assert_eq!(status, 200);
    let transects = pulled["sections"]["transects"]
        .as_array()
        .expect("transects section");
    assert_eq!(transects.len(), 1);
    assert_eq!(transects[0]["id"], transect_id);
    assert_eq!(transects[0]["name"], "Japanese Garden");
    // Attributed to the collecting installation, not to whoever minted its code.
    assert!(!transects[0]["device_id"].is_null(), "{pulled}");

    // An up-to-date client re-downloads nothing.
    let (_, again) = get_json(
        &app,
        &format!(
            "/api/sync/pull?since={}",
            pulled["cursor"].as_i64().unwrap()
        ),
        Some(&token_b),
    )
    .await;
    assert!(again["sections"].as_object().unwrap().is_empty(), "{again}");
    assert_eq!(again["has_more"], false);
}

#[tokio::test]
async fn test_conflict_resolves_last_write_wins_within_one_origin() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Alice laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect_id = "22222222-2222-4222-8222-222222222222";
    post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(transect_id, "Noon name", "2026-08-01T12:00:00Z")]
        })),
        Some(&token),
    )
    .await;

    // An older edit of the same laptop's own row must not roll it back.
    let (status, stale) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(transect_id, "Morning name", "2026-08-01T10:00:00Z")]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(stale["sections"]["transects"]["applied"], 0);
    assert_eq!(stale["sections"]["transects"]["skipped"][0], transect_id);

    let (_, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert_eq!(pulled["sections"]["transects"][0]["name"], "Noon name");

    let (_, fresh) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(transect_id, "Evening name", "2026-08-01T14:00:00Z")]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(fresh["sections"]["transects"]["applied"], 1);
    let (_, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert_eq!(pulled["sections"]["transects"][0]["name"], "Evening name");
}

#[tokio::test]
async fn test_delete_propagates_as_tombstone() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let (token_a, token_b) = two_devices(&app, &db).await;

    // Laid against a site, so the unique index the last assertion turns on is live: it is
    // scoped to (site_id, name), and NULLs are distinct in Postgres.
    let site = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
    seed_site(&db, site, "Harat").await;
    let under_site = |id: &str, name: &str, updated_at: &str| {
        let mut row = transect_row(id, name, updated_at);
        row["site_id"] = serde_json::json!(site);
        row
    };

    let transect_id = "33333333-3333-4333-8333-333333333333";
    post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [under_site(transect_id, "Doomed line", "2026-08-01T10:00:00Z")]
        })),
        Some(&token_a),
    )
    .await;

    let (_, first) = get_json(&app, "/api/sync/pull", Some(&token_b)).await;
    let cursor = first["cursor"].as_i64().unwrap();

    let mut deleted = under_site(transect_id, "Doomed line", "2026-08-01T11:00:00Z");
    deleted["deleted_at"] = serde_json::json!("2026-08-01T11:00:00Z");
    let (_, response) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "transects": [deleted] })),
        Some(&token_a),
    )
    .await;
    assert_eq!(response["sections"]["transects"]["applied"], 1);

    // As a row: an absent one is indistinguishable from one not yet seen.
    let (_, pulled) = get_json(
        &app,
        &format!("/api/sync/pull?since={cursor}"),
        Some(&token_b),
    )
    .await;
    let transects = pulled["sections"]["transects"]
        .as_array()
        .expect("tombstone arrives");
    assert_eq!(transects.len(), 1);
    assert_eq!(transects[0]["id"], transect_id);
    assert_eq!(transects[0]["deleted_at"], "2026-08-01T11:00:00Z");

    // The unique index covers live rows only, so the name frees up.
    let reborn = "44444444-4444-4444-8444-444444444444";
    let (status, reused) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [under_site(reborn, "Doomed line", "2026-08-01T12:00:00Z")]
        })),
        Some(&token_b),
    )
    .await;
    assert_eq!(status, 200, "{reused}");
    assert_eq!(
        reused["sections"]["transects"]["applied"], 1,
        "the name did not free up after the tombstone: {reused}"
    );
}

// One literal document end to end is the point, so it does not split.
#[allow(clippy::too_many_lines)]
#[tokio::test]
async fn test_push_whole_survey_in_one_call() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let site = "55555555-5555-4555-8555-555555555555";
    let campaign = "66666666-6666-4666-8666-666666666666";
    let transect = "77777777-7777-4777-8777-777777777777";
    let video = "88888888-8888-4888-8888-888888888888";
    let pass = "99999999-9999-4999-8999-999999999999";
    let run = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    let stamps = ("2026-08-01T00:00:00Z", "2026-08-01T10:00:00Z");

    // Curated on the server, and carried up by the laptop only so its children resolve.
    seed_site(&db, site, "Harat").await;
    seed_campaign(&db, campaign, "2025_10_eritrea").await;

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "sites": [site_row(site, "Harat", stamps.1)],
            "campaigns": [{
                "id": campaign, "name": "2025_10_eritrea", "description": "",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "transects": [{
                "id": transect, "site_id": site, "name": "T1", "description": "",
                "start_lat": 16.202559, "start_lon": 39.443041,
                "end_lat": 16.20168, "end_lon": 39.442997,
                "length_m": 100.0, "depth_m": 8.5,
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "videos": [{
                "id": video, "hash": "deadbeef", "file_name": "GX010297.MP4",
                "size_bytes": 4_000_000_000_i64, "duration_s": 353.0, "fps": 30.0,
                "width": 1920, "height": 1080, "codec": "hvc1",
                "gravity": "yes", "gps": "yes",
                // A column this contract no longer carries. A device on an older
                // build still sends it, and the push must ignore the key rather
                // than refuse the row.
                "sha256": "b5bb9d8014a0f9b1d61e21e796d78dccdf1352f23cd32812f4850b878ae4944c",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "passes": [{
                "id": pass, "transect_id": transect, "campaign_id": campaign,
                "begin_s": 18.0, "end_s": 353.0, "direction": "forward",
                "upside_down": false, "label": "", "notes": "", "quality": "excellent",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "pass_videos": [{
                "id": "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb",
                "pass_id": pass, "video_id": video, "ordinal": 0,
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "runs": [{
                "id": run, "pass_id": pass, "status": "succeeded",
                "started_at": stamps.1, "finished_at": "2026-08-01T10:42:00Z",
                "error": "", "run_dir_name": "run_20260801_100000",
                "gui_version": "0.9.0", "library_version": "0.14.2",
                "segmentation_model": "coralscapes-vit-b-dpt", "mapping_backend": "loger",
                "processing_width": 640, "processing_height": 352,
                "fps": 4, "preprocess_batch_size": 8,
                "taxonomy_version": 3, "taxonomy_hash": "sha256:abcd",
                "model_revisions": { "EPFL-ECEO/coralscapes-vit-b-dpt": "a1b2c3" },
                "preset_name": "eceo-default", "preset_deviations": { "fps": 4 },
                "preset_version": 2, "preset_hash": "sha256:ef01",
                "run_duration_s": 2520.5,
                "stage_durations": { "mapping": 1800.0, "segmentation": 600.0 },
                "stage_peaks": { "mapping": { "vram_mb": 7168 } },
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "cover_rows": [{
                "id": "cccccccc-cccc-4ccc-8ccc-cccccccccccc",
                "run_id": run, "level": "intermediate", "class_group": "sand",
                "estimator": "per_pass", "fraction": 0.234_024_92,
                "point_count": 120_345.0, "denominator": 514_243.0,
                "metric_source": "unprojected",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
        })),
        Some(&token),
    )
    .await;

    assert_eq!(status, 200, "push failed: {body}");
    for section in [
        "transects",
        "videos",
        "passes",
        "pass_videos",
        "runs",
        "cover_rows",
    ] {
        assert_eq!(
            body["sections"][section]["applied"], 1,
            "{section} not applied: {body}"
        );
    }
    // The two the console owns travel as ancestors: unchanged, so acknowledged and
    // nothing proposed.
    for section in ["sites", "campaigns"] {
        assert_eq!(body["sections"][section]["applied"], 1, "{body}");
        assert!(
            body["sections"][section]["refused"]
                .as_array()
                .expect("refused list")
                .is_empty(),
            "{body}"
        );
    }
    let proposals: i64 = one_value(
        &db,
        "SELECT COUNT(*)::BIGINT FROM change_log WHERE status = 'proposed'",
    )
    .await;
    assert_eq!(proposals, 0, "an unchanged ancestor became a proposal");

    // Provenance survives the round trip intact. Read as an operator: runs and cover are
    // upload only, so a device never pulls them back.
    let admin = build_test_app_as_admin(db.clone());
    let (_, pulled) = get_json(&admin, "/api/sync/pull", None).await;
    let run_row = &pulled["sections"]["runs"][0];
    assert_eq!(run_row["taxonomy_hash"], "sha256:abcd");
    assert_eq!(
        run_row["model_revisions"]["EPFL-ECEO/coralscapes-vit-b-dpt"],
        "a1b2c3"
    );
    assert_eq!(run_row["preset_deviations"]["fps"], 4);
    assert_eq!(run_row["preset_version"], 2);
    assert_eq!(run_row["preset_hash"], "sha256:ef01");
    assert_eq!(run_row["run_duration_s"], 2520.5);
    assert_eq!(run_row["processing_width"], 640);
    assert_eq!(run_row["processing_height"], 352);
    assert_eq!(run_row["fps"], 4);
    assert_eq!(run_row["preprocess_batch_size"], 8);
    assert_eq!(run_row["stage_durations"]["mapping"], 1800.0);
    assert_eq!(run_row["stage_peaks"]["mapping"]["vram_mb"], 7168);
    assert_eq!(pulled["sections"]["videos"][0]["hash"], "deadbeef");
    assert!(
        pulled["sections"]["videos"][0].get("sha256").is_none(),
        "a key outside the contract is dropped, not stored"
    );
    assert_eq!(pulled["sections"]["cover_rows"][0]["class_group"], "sand");
}

/// A build that does not report its processing configuration leaves the four columns
/// null, rather than the push inventing a resolution the run never ran at. The
/// performance aggregate pools those runs on that nullness, so it has to be real.
#[tokio::test]
async fn test_push_omitting_processing_config_stores_nulls() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Older laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect = "5a5a5a5a-5a5a-4a5a-8a5a-5a5a5a5a5a5a";
    let pass = "5b5b5b5b-5b5b-4b5b-8b5b-5b5b5b5b5b5b";
    let run = "5c5c5c5c-5c5c-4c5c-8c5c-5c5c5c5c5c5c";
    let stamps = ("2026-08-01T00:00:00Z", "2026-08-01T10:00:00Z");

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(transect, "T1", stamps.1)],
            "passes": [{
                "id": pass, "transect_id": transect,
                "begin_s": 0.0, "end_s": 60.0,
                "upside_down": false, "label": "", "notes": "",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
            "runs": [{
                "id": run, "pass_id": pass, "status": "succeeded",
                "error": "", "run_dir_name": "run_20260801_100000",
                "segmentation_model": "segformer-b2", "mapping_backend": "scsfmlearner",
                "created_at": stamps.0, "updated_at": stamps.1,
            }],
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["runs"]["applied"], 1, "{body}");

    let admin = build_test_app_as_admin(db.clone());
    let (_, pulled) = get_json(&admin, "/api/sync/pull", None).await;
    let run_row = &pulled["sections"]["runs"][0];
    assert_eq!(run_row["run_dir_name"], "run_20260801_100000", "{pulled}");
    for column in [
        "processing_width",
        "processing_height",
        "fps",
        "preprocess_batch_size",
    ] {
        assert!(
            run_row[column].is_null(),
            "{column} was invented: {run_row}"
        );
    }
}

#[tokio::test]
async fn test_push_child_without_parent_conflicts() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let orphan = "dddddddd-dddd-4ddd-8ddd-dddddddddddd";
    let good = "12341234-1234-4234-8234-123412341234";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(good, "T1", "2026-08-01T10:00:00Z")],
            "passes": [{
                "id": orphan,
                "transect_id": "eeeeeeee-eeee-4eee-8eee-eeeeeeeeeeee",
                "begin_s": 0.0, "end_s": 10.0, "direction": "forward",
                "upside_down": false, "label": "", "notes": "",
                "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
            }]
        })),
        Some(&token),
    )
    .await;

    // Named, not fatal. A document-level refusal would repeat on every later push,
    // since the client re-sends every ancestor.
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sections"]["passes"]["conflicted"][0], orphan,
        "{body}"
    );
    assert_eq!(body["sections"]["passes"]["applied"], 0, "{body}");
    assert_eq!(
        body["sections"]["transects"]["applied"], 1,
        "one bad row took the rest of the document with it: {body}"
    );

    // The laptop is not wedged: dropping the orphan is enough to sync again.
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [transect_row(good, "T1 renamed", "2026-08-01T11:00:00Z")]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["transects"]["applied"], 1, "{body}");
}

/// A document the server cannot parse is refused entire, since applying part of it would
/// silently drop rows the client believes it handed over. A row the database will not take
/// is named instead, because the client re-sends it on every push.
#[tokio::test]
async fn test_a_malformed_document_is_refused_whole_and_a_bad_row_alone() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let valid = transect_row(
        "ffffffff-ffff-4fff-8fff-ffffffffffff",
        "Valid",
        "2026-08-01T10:00:00Z",
    );

    // The valid row must not land: the client believes it handed over both.
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "transects": [valid.clone()], "gremlins": [] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    // Only the migration's seeded preset comes back, never the pushed transect.
    let (_, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert!(
        pulled["sections"]["transects"].is_null(),
        "partial write: {pulled}"
    );

    // A contract version this server does not speak.
    let (status, _) = post_json(
        &app,
        "/api/sync/push",
        &serde_json::json!({ "contract_version": 99, "sections": {} }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400);

    // A value outside a CHECK constraint is the row's own problem, so it is named and
    // the document still stands.
    let bad_direction = serde_json::json!({
        "id": "10101010-1010-4010-8010-101010101010",
        "begin_s": 0.0, "end_s": 10.0, "direction": "sideways",
        "upside_down": false, "label": "", "notes": "",
        "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
    });
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "passes": [bad_direction] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sections"]["passes"]["conflicted"][0], "10101010-1010-4010-8010-101010101010",
        "{body}"
    );

    // A nullable vocabulary still refuses a value that is not one of its codes.
    let bad_source = serde_json::json!({
        "id": "20202020-2020-4020-8020-202020202020",
        "file_name": "GX010001.MP4", "captured_source": "guessed",
        "gravity": "unknown", "gps": "unknown",
        "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
    });
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "videos": [bad_source] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sections"]["videos"]["conflicted"][0], "20202020-2020-4020-8020-202020202020",
        "{body}"
    );
}

/// Expected behaviour: an unrecorded direction stays null rather than becoming `forward`.
#[tokio::test]
async fn test_push_pass_without_direction_stores_null() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let pass = "30303030-3030-4030-8030-303030303030";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "passes": [{
                "id": pass, "begin_s": 0.0, "end_s": 10.0,
                "upside_down": false, "label": "", "notes": "",
                "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
            }]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let stored: Option<String> = one_value(
        &db,
        &format!("SELECT direction FROM transect_pass WHERE id = '{pass}'"),
    )
    .await;
    assert_eq!(stored, None);
}

#[tokio::test]
async fn test_push_ignores_client_server_seq() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let mut row = transect_row(
        "12121212-1212-4212-8212-121212121212",
        "Site",
        "2026-08-01T10:00:00Z",
    );
    // Otherwise a client could hide its row from everyone else's pull.
    row["server_seq"] = serde_json::json!(999_999);

    let (status, _) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "transects": [row] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200);

    let (_, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert!(
        pulled["cursor"].as_i64().unwrap() < 999_999,
        "client-supplied server_seq was honoured: {pulled}"
    );
}

/// Scenario: an already-shipped desktop build pushes a row still carrying
/// `created_by`, a column the contract has since dropped, and a forged `device_id`.
///
/// Expected behaviour: keys outside the contract are ignored, never refused, and the
/// credential overrides the forged provenance.
#[tokio::test]
async fn test_push_from_a_device_attributes_the_device_only() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop 1").await;
    let token = enrol_device(&app, &code).await;

    let transect_id = "77777777-7777-4777-8777-777777777777";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [{
                "id": transect_id,
                "name": "Harat",
                "description": "",
                "start_lat": 16.202559, "start_lon": 39.443041,
                "end_lat": 16.20168, "end_lon": 39.442997,
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-01T10:00:00Z",
                "created_by": "admin-sub",
                "device_id": "00000000-0000-4000-8000-000000000000",
            }]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sections"]["transects"]["applied"], 1,
        "an old client's extra key was refused rather than ignored: {body}"
    );

    let device_id: String = one_value(&db, "SELECT id::text FROM device").await;
    let stamped: String = one_value(
        &db,
        &format!("SELECT device_id::text FROM transect WHERE id = '{transect_id}'"),
    )
    .await;
    assert_eq!(stamped, device_id);
}

#[tokio::test]
async fn test_push_refuses_a_person() {
    let db = setup_test_db().await;

    // Push binds provenance from the credential and resolves conflicts on the client's
    // clock, so reaching it as a person is write and delete over every console row.
    let site_id = "88888888-8888-4888-8888-888888888888";
    let document = push_body(&serde_json::json!({
        "sites": [site_row(site_id, "Harat", "2026-08-01T10:00:00Z")]
    }));

    for (who, app) in [
        ("member", build_test_app_as_member(db.clone())),
        ("admin", build_test_app_as_admin(db.clone())),
    ] {
        let (status, body) = post_json(&app, "/api/sync/push", &document, None).await;
        assert_eq!(status, 403, "a {who} reached the ingest: {body}");
    }

    let rows: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(rows, 0, "a refused push still wrote");
}

#[tokio::test]
async fn test_paged_pull_returns_every_row_across_sections() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    // Sites are first in foreign-key order, so a shared row budget spent on them would
    // strand the campaign behind a cursor that had already moved past it.
    for n in 0..12 {
        seed_site(
            &db,
            &format!("aaaaaaaa-0000-4000-8000-{n:012}"),
            &format!("Site {n}"),
        )
        .await;
    }
    let campaign_id = "cccccccc-0000-4000-8000-000000000001";
    seed_campaign(&db, campaign_id, "2025_10_eritrea").await;

    // A laptop walks the whole feed two rows at a time.
    let code_b = seed_connect_code(&db, "bob", "Bob laptop").await;
    let token_b = enrol_device(&app, &code_b).await;

    let mut cursor = 0i64;
    let mut seen_sites = std::collections::HashSet::new();
    let mut seen_campaigns = std::collections::HashSet::new();
    for _ in 0..40 {
        let (status, page) = get_json(
            &app,
            &format!("/api/sync/pull?since={cursor}&limit=2"),
            Some(&token_b),
        )
        .await;
        assert_eq!(status, 200, "{page}");
        for row in page["sections"]["sites"].as_array().unwrap_or(&vec![]) {
            seen_sites.insert(row["id"].as_str().unwrap().to_string());
        }
        for row in page["sections"]["campaigns"].as_array().unwrap_or(&vec![]) {
            seen_campaigns.insert(row["id"].as_str().unwrap().to_string());
        }
        cursor = page["cursor"].as_i64().unwrap();
        if !page["has_more"].as_bool().unwrap() {
            break;
        }
    }

    assert_eq!(seen_sites.len(), 12, "paging lost sites");
    assert!(
        seen_campaigns.contains(campaign_id),
        "paging stranded the campaign behind the sites"
    );
}

#[tokio::test]
async fn test_unique_collision_refuses_one_row_not_the_document() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let (token_a, token_b) = two_devices(&app, &db).await;

    // The unique index is scoped to the site, and NULLs are distinct in Postgres, so a
    // collision needs both lines laid against the same reef.
    let site = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
    seed_site(&db, site, "Harat").await;
    let under_site = |id: &str, name: &str, updated_at: &str| {
        let mut row = transect_row(id, name, updated_at);
        row["site_id"] = serde_json::json!(site);
        row
    };

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [under_site("11111111-1111-4111-8111-111111111111", "T1", "2026-08-01T10:00:00Z")]
        })),
        Some(&token_a),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    // The same line under a different id, alongside a row that must still land.
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [
                under_site("22222222-2222-4222-8222-222222222222", "T1", "2026-08-02T10:00:00Z"),
                under_site("33333333-3333-4333-8333-333333333333", "T2", "2026-08-02T10:00:00Z"),
            ]
        })),
        Some(&token_b),
    )
    .await;
    assert_eq!(status, 200, "one collision failed the document: {body}");
    assert_eq!(
        body["sections"]["transects"]["conflicted"][0],
        "22222222-2222-4222-8222-222222222222"
    );
    assert_eq!(body["sections"]["transects"]["applied"], 1);

    let names: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM transect").await;
    assert_eq!(names, 2, "the surviving row did not land");
}

#[tokio::test]
async fn test_pull_serves_presets_to_a_device() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    // Authored in the console, like every preset.
    let admin = build_test_app_as_admin(db.clone());
    let settings = serde_json::json!({ "fps": 4, "processing_width": 852, "tsdf": false });
    let (status, created) = post_json(
        &admin,
        "/api/presets",
        &serde_json::json!({
            "name": "eceo-default",
            "version": 2,
            "settings": settings,
            "description": "Field defaults",
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "preset create failed: {created}");

    let (status, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert_eq!(status, 200);
    let presets = pulled["sections"]["presets"]
        .as_array()
        .expect("presets section");
    // The migration seeds one preset, so the pull carries it alongside ours.
    assert_eq!(presets.len(), 2, "{pulled}");
    let ours = presets
        .iter()
        .find(|p| p["id"] == created["id"])
        .expect("the created preset is served");
    assert_eq!(ours["name"], "eceo-default");
    assert_eq!(ours["version"], 2);
    // The settings document round-trips exactly, nested values included.
    assert_eq!(ours["settings"], settings, "{pulled}");
}

/// `presets` is pull only: a device naming the section has every row refused, and the
/// rest of its document still applies.
#[tokio::test]
async fn test_push_refuses_the_presets_section() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let preset = "40404040-4040-4040-8040-404040404040";
    let transect = "50505050-5050-4050-8050-505050505050";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "presets": [{
                "id": preset, "name": "invented", "version": 1,
                "settings": { "fps": 99 }, "description": "",
                "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
            }],
            "transects": [transect_row(transect, "T1", "2026-08-01T10:00:00Z")],
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["presets"]["applied"], 0, "{body}");
    assert_eq!(body["sections"]["presets"]["refused"][0], preset, "{body}");
    assert_eq!(body["sections"]["transects"]["applied"], 1, "{body}");

    // Only the migration's seeded preset remains.
    let presets: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM preset").await;
    assert_eq!(presets, 1, "a device authored a preset");
}
