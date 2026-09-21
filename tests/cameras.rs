//! Publishing a calibration from a laptop, and what the registry then holds.
//!
//! A device never authors a camera row through the change ledger: it posts what it
//! measured, and the console decides which laptops run under it.

#[allow(dead_code)]
mod common;

use common::*;

use deepreefmap_api::routes::private::sync::schema::CONTRACT_VERSION;

const UPLOAD: &str = "/api/camera_calibrations/upload";

fn document(name: &str, focal: f64) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "source": "colmap_radial_v1",
        "distorted": {
            "model": "RADIAL",
            "params": {"fx": focal, "fy": focal, "cx": 960.0, "cy": 540.0, "k1": 0.36, "k2": 0.24},
        },
        "rectified_pinhole": {
            "image_size": [1920, 1080],
            "K": [[focal, 0.0, 959.5], [0.0, focal, 539.5], [0.0, 0.0, 1.0]],
        },
    })
}

async fn enrolled(db: &sea_orm::DatabaseConnection) -> (axum::Router, String) {
    let app = build_test_app(db.clone());
    let code = seed_connect_code(db, "alice", "Alice laptop").await;
    let token = enrol_device(&app, &code).await;
    (app, token)
}

async fn publish(
    app: &axum::Router,
    token: &str,
    body: &serde_json::Value,
) -> (u16, serde_json::Value) {
    let (status, text) = post(app, UPLOAD, body, Some(token)).await;
    let parsed = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
    (status, parsed)
}

#[tokio::test]
async fn test_a_laptop_publishes_a_calibration_and_the_profile_is_made_for_it() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (status, body) = publish(
        &app,
        &token,
        &serde_json::json!({
            "name": "hero12_dome",
            "document": document("hero12_dome", 1243.0),
            "source_clip": "GX010042.MP4",
            "reprojection_error_px": 0.62,
            "registered_frames": 78,
        }),
    )
    .await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(body["version"], 1);
    assert_eq!(body["created"], true);
    let name: String = one_value(
        &db,
        "SELECT name FROM camera_profile WHERE LOWER(name) = 'hero12_dome'",
    )
    .await;
    assert_eq!(name, "hero12_dome");
    let width: i32 = one_value(
        &db,
        "SELECT image_width FROM camera_calibration ORDER BY created_at DESC LIMIT 1",
    )
    .await;
    assert_eq!(width, 1920, "the image size is lifted out of the document");
}

#[tokio::test]
async fn test_publishing_the_same_document_twice_stores_one_calibration() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let body = serde_json::json!({
        "name": "hero12_dome",
        "document": document("hero12_dome", 1243.0),
    });

    let (_, first) = publish(&app, &token, &body).await;
    let (status, again) = publish(&app, &token, &body).await;

    assert_eq!(status, 200, "{again}");
    assert_eq!(again["created"], false);
    assert_eq!(
        again["camera_calibration_id"],
        first["camera_calibration_id"]
    );
}

#[tokio::test]
async fn test_a_recalibration_takes_the_next_version_and_leaves_the_last_one() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;

    let (status, second) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1301.0)}),
    )
    .await;

    assert_eq!(status, 200, "{second}");
    assert_eq!(second["version"], 2);
    let held: i64 = one_value(
        &db,
        "SELECT COUNT(*) FROM camera_calibration c JOIN camera_profile p \
         ON p.id = c.camera_profile_id WHERE LOWER(p.name) = 'hero12_dome'",
    )
    .await;
    assert_eq!(
        held, 2,
        "the calibration a run was rectified with still stands"
    );
}

#[tokio::test]
async fn test_a_document_the_pipeline_could_not_load_is_refused() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (status, body) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": {"name": "hero12_dome"}}),
    )
    .await;

    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn test_a_name_no_device_could_resolve_is_refused() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (status, body) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero 12/dome", "document": document("x", 1243.0)}),
    )
    .await;

    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn test_the_bundled_profile_is_seeded_for_a_fresh_registry() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = get_json(&app, "/api/camera_profiles", None).await;

    assert_eq!(status, 200, "{body}");
    let names: Vec<&str> = body
        .as_array()
        .expect("a page of profiles")
        .iter()
        .filter_map(|row| row["name"].as_str())
        .collect();
    assert!(names.contains(&"gopro_hero_10"), "{body}");
}

#[tokio::test]
async fn test_the_seeded_calibration_is_a_document_a_device_can_load() {
    let db = setup_test_db().await;
    let document: serde_json::Value = one_value::<serde_json::Value>(
        &db,
        "SELECT document FROM camera_calibration ORDER BY version LIMIT 1",
    )
    .await;

    assert_eq!(document["name"], "gopro_hero_10");
    assert!(document["distorted"]["params"]["fx"].is_number());
    assert_eq!(document["rectified_pinhole"]["image_size"][0], 1920);
}

#[tokio::test]
async fn test_a_device_cannot_reach_the_camera_crud_routes() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (status, _) = get(&app, "/api/camera_profiles", Some(&token)).await;

    assert_eq!(status, 403);
}

/// A run made under a published calibration names it, so the registry can say
/// what a reconstruction was rectified with rather than only what the profile
/// was called on one laptop.
#[tokio::test]
async fn test_a_run_carries_the_calibration_it_was_rectified_with() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Alice laptop").await;
    let token = enrol_device(&app, &code).await;

    let (_, published) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let calibration = published["camera_calibration_id"].as_str().unwrap();

    let pass = "b1000000-0000-4000-8000-000000000000";
    let run = "c1000000-0000-4000-8000-000000000000";
    let push = serde_json::json!({
        "contract_version": CONTRACT_VERSION,
        "sections": {
            "passes": [{
                "id": pass,
                "begin_s": 0.0,
                "end_s": 120.0,
                "upside_down": false,
                "label": "",
                "notes": "",
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-01T10:00:00Z",
            }],
            "runs": [{
                "id": run,
                "pass_id": pass,
                "status": "succeeded",
                "error": "",
                "run_dir_name": "t1__p01",
                "camera_profile": "hero12_dome",
                "camera_calibration_id": calibration,
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-01T10:00:00Z",
            }],
        }
    });
    let (status, body) = post(&app, "/api/sync/push", &push, Some(&token)).await;
    assert_eq!(status, 200, "{body}");
    let pushed: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(applied(&pushed["sections"]["runs"]), 1, "{pushed}");

    let stored: String = one_value(
        &db,
        &format!("SELECT camera_calibration_id::text FROM run_record WHERE id = '{run}'"),
    )
    .await;
    assert_eq!(stored, calibration);
}

/// The document the pipeline bundles, `deepreefmap/resources/camera_profiles/
/// gopro_hero_10.json` verbatim. `profile_payload` on a laptop produces exactly
/// this, diagnostics included.
#[allow(clippy::unreadable_literal)] // digits as the file spells them
fn bundled_gopro_hero_10() -> serde_json::Value {
    serde_json::json!({
        "name": "gopro_hero_10",
        "source": "colmap_radial_v1",
        "distorted": {
            "model": "RADIAL",
            "params": {
                "fx": 1243.6276334472113,
                "fy": 1243.6276334472113,
                "cx": 960.0,
                "cy": 540.0,
                "k1": 0.36223110184368823,
                "k2": 0.2476961799393366
            }
        },
        "rectified_pinhole": {
            "image_size": [1920, 1080],
            "K": [
                [1562.98876953125, 0.0, 959.5],
                [0.0, 1562.98876953125, 539.5],
                [0.0, 0.0, 1.0]
            ]
        },
        "diagnostics": {
            "n_input_frames": 100,
            "n_registered_images": 100,
            "mean_reprojection_error_px": 0.783395585447909,
            "camera_model": "RADIAL",
            "source_video": "redacted-example-source-video",
            "sampling_fps": 10,
            "begin_s": 12.0,
            "end_s": null,
            "valid_roi_xywh": [0, 0, 1919, 1079]
        }
    })
}

/// Publishing the profile every install already bundles must be a no-op, not a
/// second version: the seed and the packaged file are the same document.
#[tokio::test]
async fn test_publishing_the_bundled_profile_is_a_no_op() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (status, body) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "gopro_hero_10", "document": bundled_gopro_hero_10()}),
    )
    .await;

    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["created"], false,
        "the seed differs from the bundled file: {body}"
    );
    assert_eq!(body["version"], 1);
}

/// Deploying is a curator's act, separate from publishing: a laptop publishes what
/// it measured, and the console decides which measurement every laptop then runs
/// under.
#[tokio::test]
async fn test_a_curator_deploys_one_calibration_of_a_profile() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let console = build_test_app_as_member(db.clone());

    let (_, first) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let profile = first["camera_profile_id"].as_str().expect("a profile");
    let v1 = first["camera_calibration_id"]
        .as_str()
        .expect("a calibration");

    let (status, body) = put(
        &console,
        &format!("/api/camera_profiles/{profile}"),
        &serde_json::json!({"current_calibration_id": v1}),
        None,
    )
    .await;

    assert_eq!(status, 200, "{body}");
    let stored: serde_json::Value = serde_json::from_str(&body).expect("JSON");
    assert_eq!(stored["current_calibration_id"], v1);
}

/// A profile nobody has deployed follows the newest, which is what it did before
/// deploying existed: publishing a rig from the field still reaches every laptop.
#[tokio::test]
async fn test_a_first_publication_leaves_the_profile_following_the_newest() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let console = build_test_app_as_member(db.clone());

    let (_, published) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let profile = published["camera_profile_id"].as_str().expect("a profile");

    let (status, body) = get_json(&console, &format!("/api/camera_profiles/{profile}"), None).await;

    assert_eq!(status, 200, "{body}");
    assert!(body["current_calibration_id"].is_null(), "{body}");
}

/// Publishing to a deployed profile stages the measurement rather than shipping it.
/// This is the whole point: a new calibration reaches nobody until it is deployed.
#[tokio::test]
async fn test_publishing_over_a_deployed_calibration_leaves_it_deployed() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let console = build_test_app_as_member(db.clone());

    let (_, first) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let profile = first["camera_profile_id"].as_str().expect("a profile");
    let v1 = first["camera_calibration_id"]
        .as_str()
        .expect("a calibration")
        .to_string();
    let (status, body) = put(
        &console,
        &format!("/api/camera_profiles/{profile}"),
        &serde_json::json!({"current_calibration_id": v1}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (_, second) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1301.0)}),
    )
    .await;
    assert_eq!(second["version"], 2, "{second}");

    let (_, stored) = get_json(&console, &format!("/api/camera_profiles/{profile}"), None).await;
    assert_eq!(stored["current_calibration_id"], v1, "{stored}");
}

/// A profile deploys its own measurements and only those: another rig's calibration
/// would rectify every laptop's footage with the wrong lens.
#[tokio::test]
async fn test_a_profile_cannot_deploy_another_rigs_calibration() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let console = build_test_app_as_member(db.clone());

    let (_, mine) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let (_, theirs) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_flat", "document": document("hero12_flat", 980.0)}),
    )
    .await;
    let profile = mine["camera_profile_id"].as_str().expect("a profile");
    let other = theirs["camera_calibration_id"]
        .as_str()
        .expect("a calibration");

    let (status, _) = put(
        &console,
        &format!("/api/camera_profiles/{profile}"),
        &serde_json::json!({"current_calibration_id": other}),
        None,
    )
    .await;

    assert_ne!(status, 200, "another profile's calibration was deployed");
}

/// Withdrawing what a profile deploys must not strand the laptops resolving it:
/// the profile falls back to following the newest.
#[tokio::test]
async fn test_withdrawing_the_deployed_calibration_releases_the_profile() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;
    let console = build_test_app_as_admin(db.clone());

    let (_, first) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let profile = first["camera_profile_id"].as_str().expect("a profile");
    let v1 = first["camera_calibration_id"]
        .as_str()
        .expect("a calibration");
    let (status, body) = put(
        &console,
        &format!("/api/camera_profiles/{profile}"),
        &serde_json::json!({"current_calibration_id": v1}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, body) = delete(&console, &format!("/api/camera_calibrations/{v1}"), None).await;
    assert_eq!(status, 204, "{body}");

    let (_, stored) = get_json(&console, &format!("/api/camera_profiles/{profile}"), None).await;
    assert!(stored["current_calibration_id"].is_null(), "{stored}");
}

/// A device reads what to deploy; it never decides it. Deciding for every other
/// laptop is the authority the console keeps.
#[tokio::test]
async fn test_a_device_cannot_deploy_a_calibration() {
    let db = setup_test_db().await;
    let (app, token) = enrolled(&db).await;

    let (_, published) = publish(
        &app,
        &token,
        &serde_json::json!({"name": "hero12_dome", "document": document("hero12_dome", 1243.0)}),
    )
    .await;
    let profile = published["camera_profile_id"].as_str().expect("a profile");
    let calibration = published["camera_calibration_id"]
        .as_str()
        .expect("a calibration");

    let (status, _) = put(
        &app,
        &format!("/api/camera_profiles/{profile}"),
        &serde_json::json!({"current_calibration_id": calibration}),
        Some(&token),
    )
    .await;

    assert_eq!(status, 403);
}
