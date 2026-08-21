//! Naming a device at mint, assigning it a preset, and the heartbeat acknowledgement.

#[allow(dead_code)]
mod common;

use common::*;
use deepreefmap_api::contract::preset_schema;

async fn seeded_preset_id(app: &axum::Router) -> String {
    let (status, body) = get_json(app, "/api/presets", None).await;
    assert_eq!(status, 200, "{body}");
    body.as_array()
        .expect("a list of presets")
        .iter()
        .find(|p| p["name"] == "Standard reef survey" && p["version"] == 1)
        .and_then(|p| p["id"].as_str())
        .expect("the migration seeds the standard preset")
        .to_string()
}

#[tokio::test]
async fn test_the_seeded_preset_passes_its_own_schema() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let id = seeded_preset_id(&member).await;
    let (status, body) = get_json(&member, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 200, "{body}");
    preset_schema::validate_settings(&body["settings"]).expect("the seed is publishable");
    // Sync stamps come from the trigger, so devices receive the seed on first pull.
    assert!(body["server_seq"].as_i64().expect("a sequence") > 0);
}

#[tokio::test]
async fn test_enrolment_adopts_the_minted_name() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());

    let code = seed_connect_code(&db, "member-sub", "Reef laptop 3").await;
    let (status, body) = post_json(
        &device_app,
        "/api/enrol",
        &serde_json::json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["device_name"], "Reef laptop 3");

    let device_id = body["device_id"].as_str().expect("a device id");
    let (status, shown) = get_json(&member, &format!("/api/devices/{device_id}"), None).await;
    assert_eq!(status, 200, "{shown}");
    assert_eq!(shown["name"], "Reef laptop 3");
}

#[tokio::test]
async fn test_a_code_minted_without_a_name_still_enrols() {
    let db = setup_test_db().await;
    let device_app = build_test_app(db.clone());

    let code = seed_connect_code(&db, "member-sub", "").await;
    let (status, body) = post_json(
        &device_app,
        "/api/enrol",
        &serde_json::json!({ "code": code }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let name = body["device_name"].as_str().expect("a name");
    assert!(name.starts_with("Device "), "fell back to {name:?}");
}

#[tokio::test]
async fn test_mint_refuses_an_empty_device_name() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let (status, body) = post_json(
        &member,
        "/api/devices/connect-codes",
        &serde_json::json!({ "device_name": "  " }),
        None,
    )
    .await;
    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn test_assignment_and_acknowledgement_round_trip() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());

    let code = seed_connect_code(&db, "member-sub", "Struggling laptop").await;
    let (_, enrolled) = post_json(
        &device_app,
        "/api/enrol",
        &serde_json::json!({ "code": code }),
        None,
    )
    .await;
    let device_id = enrolled["device_id"]
        .as_str()
        .expect("a device id")
        .to_string();
    let token = enrolled["token"].as_str().expect("a token").to_string();

    // Nothing assigned yet: the heartbeat says so.
    let (status, body) = post_json(
        &device_app,
        "/api/sync/heartbeat",
        &serde_json::json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert!(body["assigned_preset"].is_null());

    let preset_id = seeded_preset_id(&member).await;
    let (status, body) = post_json(
        &member,
        &format!("/api/devices/{device_id}/assign-preset"),
        &serde_json::json!({ "preset_id": preset_id }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["assigned_preset_id"], serde_json::json!(preset_id));

    // The device learns the assignment and acknowledges it on the next report.
    let (status, body) = post_json(
        &device_app,
        "/api/sync/heartbeat",
        &serde_json::json!({ "preset_schema_version": 1 }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["assigned_preset"]["name"], "Standard reef survey");
    assert_eq!(body["assigned_preset"]["version"], 1);

    let (status, _) = post_json(
        &device_app,
        "/api/sync/heartbeat",
        &serde_json::json!({
            "active_preset_name": "Standard reef survey",
            "active_preset_version": 1,
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200);

    let (_, shown) = get_json(&member, &format!("/api/devices/{device_id}"), None).await;
    assert_eq!(shown["active_preset_name"], "Standard reef survey");
    assert_eq!(shown["active_preset_version"], 1);
    assert_eq!(shown["preset_schema_version"], 1);
    assert!(!shown["active_preset_reported_at"].is_null());

    // Clearing the assignment resets the acknowledgement, so the console never
    // shows a stale ack against the next assignment.
    let (status, _) = post_json(
        &member,
        &format!("/api/devices/{device_id}/assign-preset"),
        &serde_json::json!({ "preset_id": null }),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (_, shown) = get_json(&member, &format!("/api/devices/{device_id}"), None).await;
    assert!(shown["assigned_preset_id"].is_null());
    assert!(shown["active_preset_name"].is_null());
    let (_, body) = post_json(
        &device_app,
        "/api/sync/heartbeat",
        &serde_json::json!({}),
        Some(&token),
    )
    .await;
    assert!(body["assigned_preset"].is_null());
}

#[tokio::test]
async fn test_assign_refuses_someone_elses_device_and_missing_presets() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());

    let code = seed_connect_code(&db, "someone-else", "Another laptop").await;
    let (_, enrolled) = post_json(
        &device_app,
        "/api/enrol",
        &serde_json::json!({ "code": code }),
        None,
    )
    .await;
    let device_id = enrolled["device_id"]
        .as_str()
        .expect("a device id")
        .to_string();

    let preset_id = seeded_preset_id(&member).await;
    let (status, _) = post_json(
        &member,
        &format!("/api/devices/{device_id}/assign-preset"),
        &serde_json::json!({ "preset_id": preset_id }),
        None,
    )
    .await;
    assert_eq!(status, 403, "a member may only assign to their own devices");

    let admin = build_test_app_as_admin(db.clone());
    let (status, _) = post_json(
        &admin,
        &format!("/api/devices/{device_id}/assign-preset"),
        &serde_json::json!({ "preset_id": "00000000-0000-0000-0000-000000000000" }),
        None,
    )
    .await;
    assert_eq!(status, 404, "an unknown preset cannot be assigned");
}

#[tokio::test]
async fn test_preset_create_refuses_what_a_laptop_could_not_run() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());

    let refused = [
        serde_json::json!({ "fps": 0 }),
        serde_json::json!({ "segmentation_name": "coralscapes-vit-xl-dpt" }),
        serde_json::json!({ "loger_model_path": "/home/someone/latest.pt" }),
        serde_json::json!({ "enable_tsdf": "yes" }),
    ];
    for settings in &refused {
        let (status, body) = post_json(
            &member,
            "/api/presets",
            &serde_json::json!({
                "name": "Bad settings", "version": 1, "settings": settings,
                "description": "",
            }),
            None,
        )
        .await;
        assert_eq!(status, 422, "accepted {settings}: {body}");
    }

    // Unknown keys pass: devices ignore what they do not recognise.
    let (status, body) = post_json(
        &member,
        "/api/presets",
        &serde_json::json!({
            "name": "Future settings", "version": 1,
            "settings": { "a_future_setting": 12, "fps": 5 },
            "description": "",
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "{body}");
}
