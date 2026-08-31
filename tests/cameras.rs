//! Publishing a calibration from a laptop, and what the registry then holds.
//!
//! A device never authors a camera row through the change ledger: it posts what it
//! measured, and the console decides which laptops run under it.

#[allow(dead_code)]
mod common;

use common::*;

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
    assert_eq!(again["camera_calibration_id"], first["camera_calibration_id"]);
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
    assert_eq!(held, 2, "the calibration a run was rectified with still stands");
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
