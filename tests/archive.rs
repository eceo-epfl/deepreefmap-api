//! The blob archive: who may call it, dedup, and the gated end-to-end path.
//!
//! Most tests run without any S3. They use either no archive configuration (the 503
//! path) or a dead endpoint, so any accidental S3 call fails loudly. The end-to-end
//! test needs a real `MinIO` and skips itself when the `S3_*` variables are absent:
//! `docker compose up -d minio minio-seed` in `../deepreefmap-ui` provides one.

#[allow(dead_code)]
mod common;

use common::*;
use deepreefmap_api::config::{ArchiveConfig, Config};
use sha2::Digest;

const HASH: &str = "0123456789abcdef0123456789abcdef";
const OTHER_HASH: &str = "fedcba9876543210fedcba9876543210";

/// An archive pointing at a dead endpoint: configured, but any S3 call errors.
fn dead_archive_config() -> Config {
    Config {
        archive: Some(ArchiveConfig {
            endpoint_url: "http://127.0.0.1:9".to_string(),
            bucket: "unreachable".to_string(),
            access_key: "nobody".to_string(),
            secret_key: "nothing".to_string(),
            prefix: "test".to_string(),
        }),
        ..test_config()
    }
}

async fn seed_object(db: &sea_orm::DatabaseConnection, content_hash: &str, status: &str) -> String {
    let id = uuid::Uuid::new_v4();
    let completed_at = if status == "complete" {
        "NOW()"
    } else {
        "NULL"
    };
    exec(
        db,
        &format!(
            "INSERT INTO stored_object \
             (id, content_hash, size_bytes, kind, status, s3_key, created_at, updated_at, completed_at) \
             VALUES ('{id}', '{content_hash}', 123, 'video', '{status}', \
             'test/videos/imohash/{content_hash}', NOW(), NOW(), {completed_at})"
        ),
    )
    .await;
    id.to_string()
}

async fn seed_complete_object(db: &sea_orm::DatabaseConnection, content_hash: &str) -> String {
    seed_object(db, content_hash, "complete").await
}

/// A pending two-part upload, so part validation runs without any S3 call.
async fn seed_pending_upload(db: &sea_orm::DatabaseConnection, content_hash: &str) -> String {
    let id = uuid::Uuid::new_v4();
    exec(
        db,
        &format!(
            "INSERT INTO stored_object \
             (id, content_hash, size_bytes, kind, status, s3_key, s3_upload_id, \
              part_size_bytes, created_at, updated_at) \
             VALUES ('{id}', '{content_hash}', {}, 'video', 'pending', \
             'test/videos/imohash/{content_hash}', 'upload-1', {}, NOW(), NOW())",
            48 * 1024 * 1024,
            32 * 1024 * 1024,
        ),
    )
    .await;
    id.to_string()
}

async fn seed_run_artifact(
    db: &sea_orm::DatabaseConnection,
    run_id: &str,
    relpath: &str,
    content_hash: &str,
    object_id: Option<&str>,
) {
    let linked = match object_id {
        Some(id) => format!("'{id}'"),
        None => "NULL".to_string(),
    };
    exec(
        db,
        &format!(
            "INSERT INTO run_artifact \
             (id, run_id, relpath, content_hash, stored_object_id, created_at, updated_at) \
             VALUES ('{}', '{run_id}', '{relpath}', '{content_hash}', {linked}, NOW(), NOW())",
            uuid::Uuid::new_v4(),
        ),
    )
    .await;
}

fn initiate_video(content_hash: &str) -> serde_json::Value {
    serde_json::json!({ "content_hash": content_hash, "size_bytes": 123, "kind": "video" })
}

#[tokio::test]
async fn test_every_archive_route_answers_503_when_unconfigured() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post(
        &app,
        "/api/archive/initiate",
        &initiate_video(HASH),
        Some(&token),
    )
    .await;
    assert_eq!(status, 503, "{body}");
    assert!(body.contains("S3_URL"), "the refusal names the fix: {body}");

    let object_id = uuid::Uuid::new_v4();
    let (status, _) = post(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [{ "part_number": 1, "etag": "x" }] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 503);
    let (status, _) = get(
        &app,
        &format!("/api/archive/{object_id}/download"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 503);
    let (status, _) = get(&app, &format!("/api/archive/by-hash/{HASH}"), Some(&token)).await;
    assert_eq!(status, 503);
    let (status, _) = post(
        &app,
        "/api/archive/probe",
        &serde_json::json!({ "hashes": [HASH] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 503);
    let (status, _) = post(
        &app,
        "/api/archive/runs-probe",
        &serde_json::json!({ "run_ids": [uuid::Uuid::new_v4()] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 503);
}

#[tokio::test]
async fn test_archive_routes_require_authentication() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());

    let (status, _) = post(&app, "/api/archive/initiate", &initiate_video(HASH), None).await;
    assert_eq!(status, 401);
    let object_id = uuid::Uuid::new_v4();
    let (status, _) = post(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [{ "part_number": 1, "etag": "x" }] }),
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = put_bytes(
        &app,
        &format!("/api/archive/{object_id}/parts/1"),
        vec![0u8; 16],
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = get(&app, &format!("/api/archive/{object_id}/download"), None).await;
    assert_eq!(status, 401);
    let (status, _) = get(&app, &format!("/api/archive/by-hash/{HASH}"), None).await;
    assert_eq!(status, 401);
    let (status, _) = post(
        &app,
        "/api/archive/probe",
        &serde_json::json!({ "hashes": [HASH] }),
        None,
    )
    .await;
    assert_eq!(status, 401);
    let (status, _) = post(
        &app,
        "/api/archive/runs-probe",
        &serde_json::json!({ "run_ids": [uuid::Uuid::new_v4()] }),
        None,
    )
    .await;
    assert_eq!(status, 401);
}

/// Devices and people both upload. The dedup path exercises this without S3.
#[tokio::test]
async fn test_both_principals_may_initiate_and_download() {
    let db = setup_test_db().await;
    seed_complete_object(&db, HASH).await;

    let device_app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&device_app, &code).await;
    let (status, body) = post_json(
        &device_app,
        "/api/archive/initiate",
        &initiate_video(HASH),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "complete");

    let member_app = build_test_app_with_config_as_human(
        db.clone(),
        dead_archive_config(),
        "member-sub",
        vec![deepreefmap_api::common::auth::Role::Member],
    );
    let (status, body) = post_json(
        &member_app,
        "/api/archive/initiate",
        &initiate_video(HASH),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let object_id = body["object_id"].as_str().unwrap().to_string();

    // Signing is local computation, so download works against a dead endpoint too.
    for (who, app, token) in [
        ("member", &member_app, None),
        ("device", &device_app, Some(token.as_str())),
    ] {
        let (status, body) =
            get_json(app, &format!("/api/archive/{object_id}/download"), token).await;
        assert_eq!(status, 200, "{who}: {body}");
        let url = body["url"].as_str().unwrap();
        assert!(
            url.contains(&format!("/api/archive/{object_id}/fetch?")),
            "{url}"
        );
        assert!(url.contains("sig="), "{url}");
    }
}

/// Content already archived is never uploaded again, and no S3 call is made: the
/// endpoint here is dead, so any call would answer 500.
#[tokio::test]
async fn test_dedup_short_circuits_without_s3() {
    let db = setup_test_db().await;
    let object_id = seed_complete_object(&db, HASH).await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &initiate_video(HASH),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["object_id"], object_id.as_str());
    assert_eq!(body["status"], "complete");
    assert!(body["upload_id"].is_null());
    assert!(body["parts_done"].as_array().unwrap().is_empty());

    let (status, body) =
        get_json(&app, &format!("/api/archive/by-hash/{HASH}"), Some(&token)).await;
    assert_eq!(status, 200);
    assert_eq!(body["object_id"], object_id.as_str());
    assert_eq!(body["status"], "complete");
    assert!(!body["completed_at"].is_null());

    let other = HASH.replace('0', "9");
    let (status, _) = get(&app, &format!("/api/archive/by-hash/{other}"), Some(&token)).await;
    assert_eq!(status, 404);
}

/// An artefact initiate on already-archived content links the run without S3.
#[tokio::test]
async fn test_artifact_initiate_links_the_run() {
    let db = setup_test_db().await;
    let object_id = seed_complete_object(&db, HASH).await;
    seed_run(&db, "33333333-3333-4333-8333-333333333333").await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({
            "content_hash": HASH, "size_bytes": 123, "kind": "artifact",
            "run_id": "33333333-3333-4333-8333-333333333333",
            "relpath": "ortho/cover.json",
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "complete");

    let linked: String = one_value(
        &db,
        "SELECT stored_object_id::text FROM run_artifact WHERE relpath = 'ortho/cover.json'",
    )
    .await;
    assert_eq!(linked, object_id);
}

/// One request answers a whole page of badges: known hashes map to their state,
/// unknown hashes are simply absent.
#[tokio::test]
async fn test_probe_answers_many_hashes_in_one_request() {
    let db = setup_test_db().await;
    let complete_id = seed_complete_object(&db, HASH).await;
    let pending = HASH.replace('0', "8");
    seed_object(&db, &pending, "pending").await;
    let unknown = HASH.replace('0', "9");
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post_json(
        &app,
        "/api/archive/probe",
        &serde_json::json!({ "hashes": [HASH, pending, unknown] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let answered = body["states"].as_object().unwrap();
    assert_eq!(answered.len(), 2, "{body}");
    assert_eq!(answered[HASH]["object_id"], complete_id.as_str());
    assert_eq!(answered[HASH]["status"], "complete");
    assert!(!answered[HASH]["completed_at"].is_null());
    assert_eq!(answered[pending.as_str()]["status"], "pending");
    assert!(answered[pending.as_str()]["completed_at"].is_null());
    assert!(!answered.contains_key(unknown.as_str()));
}

/// Counts per run from one grouped query: total artefact rows, and how many link a
/// complete or failed object. Runs without artefact rows are absent.
#[tokio::test]
async fn test_runs_probe_counts_artifact_states() {
    let db = setup_test_db().await;
    let run_id = "33333333-3333-4333-8333-333333333333";
    seed_run(&db, run_id).await;
    let complete = seed_object(&db, HASH, "complete").await;
    let failed_hash = HASH.replace('0', "7");
    let failed = seed_object(&db, &failed_hash, "failed").await;
    let pending_hash = HASH.replace('0', "8");
    let pending = seed_object(&db, &pending_hash, "pending").await;
    seed_run_artifact(&db, run_id, "ortho/cover.json", HASH, Some(&complete)).await;
    seed_run_artifact(&db, run_id, "ortho/ortho.png", &failed_hash, Some(&failed)).await;
    seed_run_artifact(&db, run_id, "cloud.npz", &pending_hash, Some(&pending)).await;
    seed_run_artifact(
        &db,
        run_id,
        "run_manifest.json",
        &HASH.replace('0', "9"),
        None,
    )
    .await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let bare_run = uuid::Uuid::new_v4();
    let (status, body) = post_json(
        &app,
        "/api/archive/runs-probe",
        &serde_json::json!({ "run_ids": [run_id, bare_run] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let answered = body["states"].as_object().unwrap();
    assert_eq!(answered.len(), 1, "{body}");
    assert_eq!(answered[run_id]["artifacts"], 4);
    assert_eq!(answered[run_id]["complete"], 1);
    assert_eq!(answered[run_id]["failed"], 1);
    assert!(!answered.contains_key(&bare_run.to_string()));
}

#[tokio::test]
async fn test_probes_reject_oversized_and_malformed_requests() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post(
        &app,
        "/api/archive/probe",
        &serde_json::json!({ "hashes": vec![HASH; 501] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400, "501 hashes: {body}");
    let (status, body) = post(
        &app,
        "/api/archive/probe",
        &serde_json::json!({ "hashes": [HASH.to_uppercase()] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400, "uppercase hash: {body}");

    let run_ids: Vec<uuid::Uuid> = (0..201).map(|_| uuid::Uuid::new_v4()).collect();
    let (status, body) = post(
        &app,
        "/api/archive/runs-probe",
        &serde_json::json!({ "run_ids": run_ids }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400, "201 run ids: {body}");
}

#[tokio::test]
async fn test_initiate_rejects_malformed_requests() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    for (name, body) in [
        ("uppercase hash", initiate_video(&HASH.to_uppercase())),
        ("short hash", initiate_video(&HASH[..31])),
        (
            "zero size",
            serde_json::json!({ "content_hash": HASH, "size_bytes": 0, "kind": "video" }),
        ),
        (
            "unknown kind",
            serde_json::json!({ "content_hash": HASH, "size_bytes": 1, "kind": "blob" }),
        ),
        (
            "artifact without run",
            serde_json::json!({ "content_hash": HASH, "size_bytes": 1, "kind": "artifact" }),
        ),
        (
            "traversal relpath",
            serde_json::json!({
                "content_hash": HASH, "size_bytes": 1, "kind": "artifact",
                "run_id": uuid::Uuid::new_v4(), "relpath": "../escape",
            }),
        ),
    ] {
        let (status, answer) = post(&app, "/api/archive/initiate", &body, Some(&token)).await;
        assert_eq!(status, 400, "{name}: {answer}");
    }

    let (status, _) = post(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({
            "content_hash": HASH, "size_bytes": 1, "kind": "artifact",
            "run_id": uuid::Uuid::new_v4(), "relpath": "fine.json",
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 404, "an unknown run is not found, not bad request");
}

/// One overview for the console: each run once with its artefact counts and state,
/// each archived clip by file name, and the objects nothing refers to as one total.
#[tokio::test]
async fn test_overview_groups_objects_by_run_and_clip() {
    let db = setup_test_db().await;
    let run_id = "33333333-3333-4333-8333-333333333333";
    seed_run(&db, run_id).await;
    let first = seed_object(&db, HASH, "complete").await;
    let second_hash = HASH.replace('0', "7");
    let second = seed_object(&db, &second_hash, "complete").await;
    let pending_hash = HASH.replace('0', "8");
    let pending = seed_object(&db, &pending_hash, "pending").await;
    seed_run_artifact(&db, run_id, "ortho/cover.json", HASH, Some(&first)).await;
    seed_run_artifact(&db, run_id, "ortho/ortho.png", &second_hash, Some(&second)).await;
    seed_run_artifact(&db, run_id, "cloud.npz", &pending_hash, Some(&pending)).await;
    let clip_object = seed_complete_object(&db, OTHER_HASH).await;
    seed_video(&db, "44444444-4444-4444-8444-444444444444", OTHER_HASH, "GX010042.MP4").await;
    let orphan_hash = HASH.replace('0', "9");
    seed_object(&db, &orphan_hash, "complete").await;
    let app = build_test_app_with_config_as_human(
        db.clone(),
        dead_archive_config(),
        "member-sub",
        vec![deepreefmap_api::common::auth::Role::Member],
    );

    let (status, body) = get_json(&app, "/api/archive/overview", None).await;
    assert_eq!(status, 200, "{body}");
    let runs = body["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 1, "{body}");
    assert_eq!(runs[0]["run_id"], run_id);
    assert_eq!(runs[0]["run_status"], "succeeded");
    assert_eq!(runs[0]["artifacts"], 3);
    assert_eq!(runs[0]["complete"], 2);
    assert_eq!(runs[0]["failed"], 0);
    assert_eq!(runs[0]["pending"], 1);
    assert_eq!(runs[0]["size_bytes"], 3 * 123, "every linked object, pending included");
    assert_eq!(runs[0]["state"], "partial");
    assert!(runs[0]["last_completed_at"].is_string(), "{body}");

    let clips = body["clips"].as_array().unwrap();
    assert_eq!(clips.len(), 1, "{body}");
    assert_eq!(clips[0]["file_name"], "GX010042.MP4");
    assert_eq!(clips[0]["object_id"], clip_object);
    assert_eq!(clips[0]["content_hash"], OTHER_HASH);
    assert_eq!(clips[0]["status"], "complete");

    assert_eq!(body["unlinked"]["objects"], 1);
    assert_eq!(body["unlinked"]["size_bytes"], 123);
}

/// The bundle link names one group, counts what it holds, and is refused for a
/// group the run archived nothing under.
#[tokio::test]
async fn test_bundle_link_counts_the_group_it_signs() {
    let db = setup_test_db().await;
    let run_id = "55555555-5555-4555-8555-555555555555";
    seed_run(&db, run_id).await;
    let ortho = seed_complete_object(&db, HASH).await;
    let frame_hash = HASH.replace('0', "7");
    let frame = seed_complete_object(&db, &frame_hash).await;
    seed_run_artifact(&db, run_id, "ortho.png", HASH, Some(&ortho)).await;
    seed_run_artifact(&db, run_id, "frames/000001.png", &frame_hash, Some(&frame)).await;
    let app = build_test_app_with_config_as_human(
        db.clone(),
        dead_archive_config(),
        "member-sub",
        vec![deepreefmap_api::common::auth::Role::Member],
    );

    let (status, body) = get_json(&app, &format!("/api/runs/{run_id}/outputs/bundle"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["file_count"], 2, "every complete file: {body}");
    assert_eq!(body["filename"], "run-all.zip");
    let url = body["url"].as_str().expect("a signed url");
    assert!(url.contains(&format!("/archive/runs/{run_id}/outputs.zip")), "{url}");

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/bundle?purpose=Results"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["file_count"], 1, "only the ortho: {body}");
    assert_eq!(body["filename"], "run-results.zip");

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/bundle?purpose=labels"),
        None,
    )
    .await;
    assert_eq!(status, 404, "no labels were archived: {body}");
}

#[tokio::test]
async fn test_a_bundle_stream_refuses_a_wrong_signature() {
    let db = setup_test_db().await;
    let run_id = "66666666-6666-4666-8666-666666666666";
    seed_run(&db, run_id).await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());

    let (status, body) = get(
        &app,
        &format!("/api/archive/runs/{run_id}/outputs.zip?purpose=all&expires=99999999999&sig=00"),
        None,
    )
    .await;
    assert_eq!(status, 403, "{body}");
}

#[tokio::test]
async fn test_overview_refuses_devices() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = get(&app, "/api/archive/overview", Some(&token)).await;
    assert_eq!(status, 403, "a device browsed the archive: {body}");
}

async fn seed_video(db: &sea_orm::DatabaseConnection, id: &str, hash: &str, file_name: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO video_asset (id, hash, file_name, created_at, updated_at) \
             VALUES ('{id}', '{hash}', '{file_name}', NOW(), NOW())"
        ),
    )
    .await;
}

/// A pass and a run for artefacts to hang off.
async fn seed_run(db: &sea_orm::DatabaseConnection, run_id: &str) {
    exec(
        db,
        "INSERT INTO transect_pass (id, begin_s, end_s, created_at, updated_at) \
         VALUES ('22222222-2222-4222-8222-222222222222', 0, 10, NOW(), NOW())",
    )
    .await;
    exec(
        db,
        &format!(
            "INSERT INTO run_record (id, pass_id, status, run_dir_name, created_at, updated_at) \
             VALUES ('{run_id}', '22222222-2222-4222-8222-222222222222', 'succeeded', 'run', \
             NOW(), NOW())"
        ),
    )
    .await;
}

/// Everything the part route refuses before it would touch S3: the endpoint here
/// is dead, so reaching S3 would answer 500, never these statuses.
#[tokio::test]
async fn test_upload_part_validates_before_touching_s3() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let missing = uuid::Uuid::new_v4();
    let (status, _) = put_bytes(
        &app,
        &format!("/api/archive/{missing}/parts/1"),
        vec![1u8; 16],
        Some(&token),
    )
    .await;
    assert_eq!(status, 404);

    let complete_id = seed_complete_object(&db, HASH).await;
    let (status, body) = put_bytes(
        &app,
        &format!("/api/archive/{complete_id}/parts/1"),
        vec![1u8; 16],
        Some(&token),
    )
    .await;
    assert_eq!(status, 409, "{body}");

    let pending_id = seed_pending_upload(&db, OTHER_HASH).await;
    for bad_number in [0, 3] {
        let (status, body) = put_bytes(
            &app,
            &format!("/api/archive/{pending_id}/parts/{bad_number}"),
            vec![1u8; 16],
            Some(&token),
        )
        .await;
        assert_eq!(status, 400, "part {bad_number}: {body}");
    }

    let (status, body) = put_bytes(
        &app,
        &format!("/api/archive/{pending_id}/parts/1"),
        Vec::new(),
        Some(&token),
    )
    .await;
    assert_eq!(status, 413, "an empty part is refused: {body}");
}

/// A fetch link is refused when nothing signed it: no signature, a stale expiry,
/// or a pending object all answer without touching S3.
#[tokio::test]
async fn test_fetch_refuses_unsigned_and_pending() {
    let db = setup_test_db().await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());

    let object_id = seed_complete_object(&db, HASH).await;
    let (status, _, _) = get_bytes(
        &app,
        &format!("/api/archive/{object_id}/fetch?expires=9999999999&sig=abcd"),
        None,
    )
    .await;
    assert_eq!(status, 403);
    let (status, _, _) = get_bytes(&app, &format!("/api/archive/{object_id}/fetch"), None).await;
    assert_eq!(status, 400, "the parameters are not optional");

    // A link minted while the object was complete dies with a later restart.
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;
    let (status, body) = get_json(
        &app,
        &format!("/api/archive/{object_id}/download"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let path = fetch_path(body["url"].as_str().unwrap());
    exec(
        &db,
        &format!("UPDATE stored_object SET status = 'pending' WHERE id = '{object_id}'"),
    )
    .await;
    let (status, _, _) = get_bytes(&app, &path, None).await;
    assert_eq!(status, 404, "only complete objects stream");
}

// ── The gated end-to-end path ────────────────────────────────────────────

/// The archive configuration from the environment, or `None` to skip.
///
/// The prefix is randomised per call, so runs cannot see each other's objects.
fn s3_archive_config() -> Option<ArchiveConfig> {
    dotenvy::dotenv().ok();
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let url = var("S3_URL")?;
    Some(ArchiveConfig {
        endpoint_url: if url.contains("://") {
            url
        } else {
            format!("http://{url}")
        },
        bucket: var("S3_BUCKET_ID")?,
        access_key: var("S3_ACCESS_KEY")?,
        secret_key: var("S3_SECRET_KEY")?,
        prefix: format!("test-{}", uuid::Uuid::new_v4()),
    })
}

/// The path and query of a fetch link, host stripped, for the in-process router.
fn fetch_path(url: &str) -> String {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let from_path = &after_scheme[after_scheme.find('/').unwrap()..];
    from_path.to_string()
}

fn sha256_hex_of(bytes: &[u8]) -> String {
    format!("{:x}", sha2::Sha256::digest(bytes))
}

/// The imohash a device would compute for this content, which `complete` re-computes
/// from the stored object and refuses to mismatch.
fn content_hash_of(bytes: &[u8]) -> String {
    deepreefmap_api::archive::imohash::hash_bytes(bytes)
}

/// Bytes that differ per offset, so a part uploaded to the wrong slot cannot hash right.
fn patterned_bytes(len: usize) -> Vec<u8> {
    #[allow(clippy::cast_possible_truncation)]
    (0..len).map(|i| (i % 251) as u8).collect()
}

async fn archived_status(app: &axum::Router, content_hash: &str, token: &str) -> String {
    let (status, body) = get_json(
        app,
        &format!("/api/archive/by-hash/{content_hash}"),
        Some(token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    body["status"].as_str().unwrap().to_string()
}

/// PUT every part `parts_done` does not list through the API, as a client would.
async fn upload_parts(
    app: &axum::Router,
    token: &str,
    body: &serde_json::Value,
    content: &[u8],
) -> Vec<serde_json::Value> {
    let object_id = body["object_id"].as_str().unwrap();
    let part_size = usize::try_from(body["part_size_bytes"].as_i64().unwrap()).unwrap();
    let done: Vec<i64> = body["parts_done"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n.as_i64().unwrap())
        .collect();
    let count = content.len().div_ceil(part_size);
    let mut parts = Vec::new();
    for number in 1..=count {
        if done.contains(&i64::try_from(number).unwrap()) {
            continue;
        }
        let begin = (number - 1) * part_size;
        let end = (begin + part_size).min(content.len());
        let (status, answer) = put_bytes(
            app,
            &format!("/api/archive/{object_id}/parts/{number}"),
            content[begin..end].to_vec(),
            Some(token),
        )
        .await;
        assert_eq!(status, 200, "part {number}: {answer}");
        let answer: serde_json::Value = serde_json::from_str(&answer).unwrap();
        assert!(!answer["etag"].as_str().unwrap().is_empty());
        parts.push(answer);
    }
    parts
}

/// Initiate, upload, complete, download: the whole path against `MinIO`.
#[tokio::test]
async fn test_end_to_end_upload_and_download() {
    let Some(archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    // Two parts: one full 32 MiB, one small remainder.
    let content = patterned_bytes(32 * 1024 * 1024 + 512 * 1024);
    let content_hash = content_hash_of(&content);

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": content_hash, "size_bytes": content.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "pending");
    assert_eq!(body["part_size_bytes"], 32 * 1024 * 1024);
    assert!(body["parts_done"].as_array().unwrap().is_empty());
    let object_id = body["object_id"].as_str().unwrap().to_string();

    let parts = upload_parts(&app, &token, &body, &content).await;
    assert_eq!(parts.len(), 2);
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": parts }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "complete");
    assert_eq!(
        archived_status(&app, &content_hash, &token).await,
        "complete"
    );

    // Dedup now answers complete with nothing to upload.
    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": content_hash, "size_bytes": content.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "complete");

    let (status, body) = get_json(
        &app,
        &format!("/api/archive/{object_id}/download"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let path = fetch_path(body["url"].as_str().unwrap());
    // No bearer: the signature in the query is the whole credential.
    let (status, headers, fetched) = get_bytes(&app, &path, None).await;
    assert_eq!(status, 200);
    assert_eq!(
        headers["content-length"].to_str().unwrap(),
        content.len().to_string()
    );
    assert_eq!(
        sha256_hex_of(&fetched),
        sha256_hex_of(&content),
        "the round trip preserves content"
    );
}

/// The download link terminates at the registry, is signed for one object, and
/// refuses tampering. The store itself is never addressed by a client.
#[tokio::test]
async fn test_download_link_is_signed_and_api_terminated() {
    let Some(archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let content = patterned_bytes(1024 * 1024);
    let content_hash = content_hash_of(&content);

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": content_hash, "size_bytes": content.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let object_id = body["object_id"].as_str().unwrap().to_string();
    upload_parts(&app, &token, &body, &content).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = get_json(
        &app,
        &format!("/api/archive/{object_id}/download"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let url = body["url"].as_str().unwrap();
    assert!(
        url.starts_with("http://test.local/api/"),
        "the link points at the registry, never the store: {url}"
    );

    let path = fetch_path(url);
    let (status, headers, fetched) = get_bytes(&app, &path, None).await;
    assert_eq!(status, 200);
    assert_eq!(sha256_hex_of(&fetched), sha256_hex_of(&content));
    let disposition = headers["content-disposition"].to_str().unwrap();
    assert!(disposition.contains(&content_hash), "{disposition}");

    // A tampered signature or object learns nothing, not even 404 vs 403 detail.
    let sig = path.split("sig=").nth(1).unwrap();
    let tampered = path.replace(sig, &"0".repeat(sig.len()));
    let (status, _, _) = get_bytes(&app, &tampered, None).await;
    assert_eq!(status, 403);
    let other = uuid::Uuid::new_v4();
    let reused = path.replace(&object_id, &other.to_string());
    let (status, _, _) = get_bytes(&app, &reused, None).await;
    assert_eq!(status, 403, "a signature never transfers to another object");
}

/// Wrong bytes under a claimed hash are refused at `complete`: the object is
/// deleted, the row fails, and the content stays unarchived, so one bad upload
/// cannot poison the content-addressed key every other device dedups against.
#[tokio::test]
async fn test_complete_refuses_content_that_is_not_the_claimed_hash() {
    let Some(archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    let store = deepreefmap_api::archive::store::ArchiveStore::new(&archive_config);
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let real = patterned_bytes(1024 * 1024);
    let claimed_hash = content_hash_of(&real);
    let wrong = vec![0xAB_u8; real.len()];

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": claimed_hash, "size_bytes": real.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let object_id = body["object_id"].as_str().unwrap().to_string();

    upload_parts(&app, &token, &body, &wrong).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains("hashes to"),
        "the refusal names the mismatch: {error}"
    );
    assert_eq!(archived_status(&app, &claimed_hash, &token).await, "failed");
    let failure: String = one_value(
        &db,
        &format!("SELECT failure FROM stored_object WHERE content_hash = '{claimed_hash}'"),
    )
    .await;
    assert!(failure.contains("hashes to"), "{failure}");

    let key = deepreefmap_api::archive::keys::video_key(&store.prefix, &claimed_hash);
    assert_eq!(
        store.object_size(&key).await.unwrap(),
        None,
        "nothing wrong sits at the content-addressed key"
    );

    // The content is not archived: a device holding the real file starts over.
    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": claimed_hash, "size_bytes": real.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["status"], "pending");
}

/// Fewer bytes than initiated fail the size check before any hashing.
#[tokio::test]
async fn test_complete_refuses_a_short_upload() {
    let Some(archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let content = patterned_bytes(300 * 1024);
    let claimed_size = content.len() + 4096;

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": content_hash_of(&content), "size_bytes": claimed_size, "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let object_id = body["object_id"].as_str().unwrap().to_string();

    upload_parts(&app, &token, &body, &content).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 409, "{body}");
    let error = body["error"].as_str().unwrap();
    assert!(
        error.contains("bytes"),
        "the refusal names the sizes: {error}"
    );
}

/// A file under the sampling threshold verifies down the whole-file path.
#[tokio::test]
async fn test_complete_verifies_small_objects_whole() {
    let Some(archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let content = patterned_bytes(64 * 1024);
    let content_hash = content_hash_of(&content);

    let (status, body) = post_json(
        &app,
        "/api/archive/initiate",
        &serde_json::json!({ "content_hash": content_hash, "size_bytes": content.len(), "kind": "video" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let object_id = body["object_id"].as_str().unwrap().to_string();

    upload_parts(&app, &token, &body, &content).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": [] }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        archived_status(&app, &content_hash, &token).await,
        "complete"
    );
}

/// A run whose outputs cover every group the rule names, with one file per status.
async fn seed_grouped_outputs(db: &sea_orm::DatabaseConnection, run_id: &str) {
    seed_run(db, run_id).await;
    let ortho = seed_complete_object(db, HASH).await;
    let log_hash = HASH.replace('0', "5");
    let log = seed_object(db, &log_hash, "pending").await;
    let frame_hash = HASH.replace('0', "7");
    let frame = seed_complete_object(db, &frame_hash).await;
    let label_hash = HASH.replace('0', "9");
    let label = seed_object(db, &label_hash, "failed").await;
    seed_run_artifact(db, run_id, "ortho.png", HASH, Some(&ortho)).await;
    seed_run_artifact(db, run_id, "run.log", &log_hash, Some(&log)).await;
    seed_run_artifact(db, run_id, "mapping_outputs.npz", &log_hash, None).await;
    seed_run_artifact(db, run_id, "frames/000001.png", &frame_hash, Some(&frame)).await;
    seed_run_artifact(db, run_id, "frames/000002.png", &frame_hash, Some(&frame)).await;
    seed_run_artifact(db, run_id, "labels/000001.png", &label_hash, Some(&label)).await;
}

fn console(db: &sea_orm::DatabaseConnection) -> axum::Router {
    build_test_app_with_config_as_human(
        db.clone(),
        dead_archive_config(),
        "member-sub",
        vec![deepreefmap_api::common::auth::Role::Member],
    )
}

#[tokio::test]
async fn test_run_outputs_group_every_file_by_purpose() {
    let db = setup_test_db().await;
    let run_id = "77777777-7777-4777-8777-777777777777";
    seed_grouped_outputs(&db, run_id).await;
    let app = console(&db);

    let (status, body) = get_json(&app, &format!("/api/runs/{run_id}/outputs"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["files"], 6, "{body}");
    assert_eq!(body["complete"], 3, "{body}");
    assert_eq!(body["failed"], 1, "{body}");
    assert_eq!(body["pending"], 1, "{body}");
    // Five linked objects of 123 bytes; the unlinked artefact has no size of its own.
    assert_eq!(body["size_bytes"], 5 * 123, "{body}");

    let groups = body["groups"].as_array().expect("groups");
    let names: Vec<&str> = groups.iter().map(|g| g["name"].as_str().unwrap()).collect();
    assert_eq!(
        names,
        vec!["Results", "Record", "Working data", "frames", "labels"],
        "{body}"
    );
    assert_eq!(groups[0]["files"], 1);
    assert_eq!(groups[0]["complete"], 1);
    assert_eq!(groups[1]["pending"], 1, "run.log is still uploading");
    assert_eq!(groups[2]["files"], 1, "the unlinked npz is working data");
    assert_eq!(groups[2]["complete"], 0);
    assert_eq!(groups[3]["files"], 2, "both frames");
    assert_eq!(groups[4]["failed"], 1, "the label upload failed");
}

#[tokio::test]
async fn test_run_outputs_count_past_a_list_page() {
    let db = setup_test_db().await;
    let run_id = "78787878-7878-4878-8878-787878787878";
    seed_run(&db, run_id).await;
    let ortho = seed_complete_object(&db, HASH).await;
    seed_run_artifact(&db, run_id, "ortho.png", HASH, Some(&ortho)).await;
    exec(
        &db,
        &format!(
            "INSERT INTO run_artifact (id, run_id, relpath, content_hash, created_at, updated_at) \
             SELECT gen_random_uuid(), '{run_id}', \
                    'frames/' || lpad(n::text, 6, '0') || '.png', '{HASH}', NOW(), NOW() \
             FROM generate_series(1, 1100) AS n"
        ),
    )
    .await;
    let app = console(&db);

    let (status, body) = get_json(&app, &format!("/api/runs/{run_id}/outputs"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["files"], 1101, "{body}");
    let groups = body["groups"].as_array().expect("groups");
    let frames = groups.iter().find(|g| g["name"] == "frames").expect("frames");
    assert_eq!(frames["files"], 1100, "{body}");
    assert!(
        groups.iter().any(|g| g["name"] == "Results"),
        "the ortho keeps its group behind a thousand frames: {body}"
    );

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/files?purpose=frames&offset=1000&limit=200"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let files = body["files"].as_array().expect("files");
    assert_eq!(files.len(), 100, "the tail past the page cap is reachable: {body}");
    assert_eq!(files[0]["relpath"], "frames/001001.png", "{body}");
}

#[tokio::test]
async fn test_run_output_files_list_one_group() {
    let db = setup_test_db().await;
    let run_id = "79797979-7979-4979-8979-797979797979";
    seed_grouped_outputs(&db, run_id).await;
    let app = console(&db);

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/files?purpose=frames&offset=1&limit=1"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let files = body["files"].as_array().expect("files");
    assert_eq!(files.len(), 1, "{body}");
    assert_eq!(files[0]["relpath"], "frames/000002.png", "in path order");
    assert_eq!(files[0]["status"], "complete", "the status is joined");

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/files?purpose=Results"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let files = body["files"].as_array().expect("files");
    assert_eq!(files.len(), 1, "only the root result files: {body}");
    assert_eq!(files[0]["relpath"], "ortho.png");

    let (status, body) = get_json(
        &app,
        &format!("/api/runs/{run_id}/outputs/files?purpose=masks"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["files"].as_array().expect("files").is_empty(), "{body}");
}

#[tokio::test]
async fn test_run_outputs_refuse_devices() {
    let db = setup_test_db().await;
    let run_id = "7a7a7a7a-7a7a-4a7a-8a7a-7a7a7a7a7a7a";
    seed_grouped_outputs(&db, run_id).await;
    let app = build_test_app_with_config(db.clone(), dead_archive_config());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = get(&app, &format!("/api/runs/{run_id}/outputs"), Some(&token)).await;
    assert_eq!(status, 403, "{body}");
    let (status, body) = get(
        &app,
        &format!("/api/runs/{run_id}/outputs/files?purpose=frames"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 403, "{body}");
}

#[tokio::test]
async fn test_a_bundle_folder_refuses_a_traversing_run_name() {
    let db = setup_test_db().await;
    let run_id = "7b7b7b7b-7b7b-4b7b-8b7b-7b7b7b7b7b7b";
    seed_run(&db, run_id).await;
    exec(
        &db,
        &format!("UPDATE run_record SET run_dir_name = '../../evil' WHERE id = '{run_id}'"),
    )
    .await;
    let ortho = seed_complete_object(&db, HASH).await;
    seed_run_artifact(&db, run_id, "ortho.png", HASH, Some(&ortho)).await;
    let app = console(&db);

    let (status, body) = get_json(&app, &format!("/api/runs/{run_id}/outputs/bundle"), None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["filename"], "7b7b7b7b-7b7b-4b7b-8b7b-7b7b7b7b7b7b-all.zip",
        "the run id stands in for a name that could escape the folder: {body}"
    );
}

#[tokio::test]
async fn test_a_bundle_stream_fails_loudly_when_a_file_is_gone() {
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let db = setup_test_db().await;
    let run_id = "7c7c7c7c-7c7c-4c7c-8c7c-7c7c7c7c7c7c";
    seed_run(&db, run_id).await;
    let ortho = seed_complete_object(&db, HASH).await;
    seed_run_artifact(&db, run_id, "ortho.png", HASH, Some(&ortho)).await;
    let app = console(&db);

    let (status, body) = get_json(&app, &format!("/api/runs/{run_id}/outputs/bundle"), None).await;
    assert_eq!(status, 200, "{body}");
    let url = body["url"].as_str().expect("a signed url");
    let at = url.find("/archive/runs/").expect("a bundle path");
    let path = format!("/api{}", &url[at..]);

    // The store is unreachable, so packing fails after the response has begun.
    let response = app
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri(&path)
                .body(axum::body::Body::empty())
                .expect("request builds"),
        )
        .await
        .expect("router responds");
    assert_eq!(response.status(), 200);
    assert!(
        response.into_body().collect().await.is_err(),
        "a zip that never got its central directory must not read as a whole file"
    );
}
