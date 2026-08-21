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

/// An archive pointing at a dead endpoint: configured, but any S3 call errors.
fn dead_archive_config() -> Config {
    Config {
        archive: Some(ArchiveConfig {
            endpoint_url: "http://127.0.0.1:9".to_string(),
            public_endpoint_url: None,
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

    // Presigning is local computation, so download works against a dead endpoint too.
    for (who, app, token) in [
        ("member", &member_app, None),
        ("device", &device_app, Some(token.as_str())),
    ] {
        let (status, body) =
            get_json(app, &format!("/api/archive/{object_id}/download"), token).await;
        assert_eq!(status, 200, "{who}: {body}");
        assert!(body["url"].as_str().unwrap().contains(HASH));
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
    assert!(body["part_urls"].as_array().unwrap().is_empty());
    assert!(body["upload_id"].is_null());

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
        public_endpoint_url: None,
        bucket: var("S3_BUCKET_ID")?,
        access_key: var("S3_ACCESS_KEY")?,
        secret_key: var("S3_SECRET_KEY")?,
        prefix: format!("test-{}", uuid::Uuid::new_v4()),
    })
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

async fn upload_parts(
    client: &reqwest::Client,
    body: &serde_json::Value,
    content: &[u8],
) -> Vec<serde_json::Value> {
    let part_size = usize::try_from(body["part_size_bytes"].as_i64().unwrap()).unwrap();
    let mut parts = Vec::new();
    for part in body["part_urls"].as_array().unwrap() {
        let number = part["part_number"].as_i64().unwrap();
        let begin = (usize::try_from(number).unwrap() - 1) * part_size;
        let end = (begin + part_size).min(content.len());
        let sent = client
            .put(part["url"].as_str().unwrap())
            .body(content[begin..end].to_vec())
            .send()
            .await
            .expect("the part uploads");
        assert!(
            sent.status().is_success(),
            "part {number}: {}",
            sent.status()
        );
        let etag = sent
            .headers()
            .get("etag")
            .expect("S3 answers an ETag")
            .to_str()
            .unwrap()
            .to_string();
        parts.push(serde_json::json!({ "part_number": number, "etag": etag }));
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
    let client = reqwest::Client::new();

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
    assert_eq!(body["part_urls"].as_array().unwrap().len(), 2);
    assert!(body["parts_done"].as_array().unwrap().is_empty());
    let object_id = body["object_id"].as_str().unwrap().to_string();

    let parts = upload_parts(&client, &body, &content).await;
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
    let fetched = client
        .get(body["url"].as_str().unwrap())
        .send()
        .await
        .expect("the download URL answers")
        .bytes()
        .await
        .expect("the body reads");
    assert_eq!(
        sha256_hex_of(&fetched),
        sha256_hex_of(&content),
        "the round trip preserves content"
    );
}

/// A public endpoint signs every URL a client touches while bucket operations stay
/// on the primary. The same `MinIO` answers under both names, so the whole path runs:
/// uploads land through the public URLs and assembly runs through the primary.
#[tokio::test]
async fn test_public_endpoint_signs_client_urls() {
    let Some(mut archive_config) = s3_archive_config() else {
        eprintln!("skipping: S3_URL/S3_BUCKET_ID/S3_ACCESS_KEY/S3_SECRET_KEY not set");
        return;
    };
    // localhost and 127.0.0.1 reach one MinIO, so the split is observable locally.
    let public = if archive_config.endpoint_url.contains("127.0.0.1") {
        archive_config
            .endpoint_url
            .replace("127.0.0.1", "localhost")
    } else {
        archive_config
            .endpoint_url
            .replace("localhost", "127.0.0.1")
    };
    archive_config.public_endpoint_url = Some(public.clone());
    let db = setup_test_db().await;
    let config = Config {
        archive: Some(archive_config),
        ..test_config()
    };
    let app = build_test_app_with_config(db.clone(), config);
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;
    let client = reqwest::Client::new();

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
    for part in body["part_urls"].as_array().unwrap() {
        let url = part["url"].as_str().unwrap();
        assert!(
            url.starts_with(&public),
            "signed against the primary: {url}"
        );
    }
    let object_id = body["object_id"].as_str().unwrap().to_string();

    let parts = upload_parts(&client, &body, &content).await;
    let (status, body) = post_json(
        &app,
        &format!("/api/archive/{object_id}/complete"),
        &serde_json::json!({ "parts": parts }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        archived_status(&app, &content_hash, &token).await,
        "complete"
    );

    let (status, body) = get_json(
        &app,
        &format!("/api/archive/{object_id}/download"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let url = body["url"].as_str().unwrap();
    assert!(
        url.starts_with(&public),
        "signed against the primary: {url}"
    );
    let fetched = client
        .get(url)
        .send()
        .await
        .expect("the download URL answers")
        .bytes()
        .await
        .expect("the body reads");
    assert_eq!(sha256_hex_of(&fetched), sha256_hex_of(&content));
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
    let client = reqwest::Client::new();

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

    upload_parts(&client, &body, &wrong).await;
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
    let client = reqwest::Client::new();

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

    upload_parts(&client, &body, &content).await;
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
    let client = reqwest::Client::new();

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

    upload_parts(&client, &body, &content).await;
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
