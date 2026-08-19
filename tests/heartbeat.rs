//! Device self-reports: what `/api/sync/heartbeat` stores, and who it refuses.

#[allow(dead_code)]
mod common;

use common::*;

#[tokio::test]
async fn test_heartbeat_updates_the_calling_device() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    let (status, body) = post(
        &app,
        "/api/sync/heartbeat",
        &serde_json::json!({
            "gui_version": "0.10.0",
            "library_version": "0.15.0",
            "platform": "windows",
            "system_profile": { "gpu": "RTX 4070 Laptop", "ram_gb": 32 },
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 204, "{body}");

    let gui: Option<String> = one_value(&db, "SELECT gui_version FROM device").await;
    assert_eq!(gui.as_deref(), Some("0.10.0"));
    let library: Option<String> = one_value(&db, "SELECT library_version FROM device").await;
    assert_eq!(library.as_deref(), Some("0.15.0"));
    let platform: Option<String> = one_value(&db, "SELECT platform FROM device").await;
    assert_eq!(platform.as_deref(), Some("windows"));
    let gpu: Option<String> = one_value(&db, "SELECT system_profile->>'gpu' FROM device").await;
    assert_eq!(gpu.as_deref(), Some("RTX 4070 Laptop"));
    let stamped: i64 = one_value(
        &db,
        "SELECT COUNT(*)::BIGINT FROM device WHERE profile_reported_at IS NOT NULL",
    )
    .await;
    assert_eq!(stamped, 1);
}

/// A field in a report is an update; a field left out is not a retraction.
#[tokio::test]
async fn test_heartbeat_leaves_absent_fields_alone() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;
    exec(&db, "UPDATE device SET gui_version = '0.9.0'").await;

    let (status, body) = post(
        &app,
        "/api/sync/heartbeat",
        &serde_json::json!({ "library_version": "0.15.0" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 204, "{body}");

    let gui: Option<String> = one_value(&db, "SELECT gui_version FROM device").await;
    assert_eq!(gui.as_deref(), Some("0.9.0"), "a partial report erased");
    let library: Option<String> = one_value(&db, "SELECT library_version FROM device").await;
    assert_eq!(library.as_deref(), Some("0.15.0"));
}

#[tokio::test]
async fn test_heartbeat_refuses_a_person() {
    let db = setup_test_db().await;
    let document = serde_json::json!({ "gui_version": "0.10.0" });

    for (who, app) in [
        ("member", build_test_app_as_member(db.clone())),
        ("admin", build_test_app_as_admin(db.clone())),
    ] {
        let (status, body) = post(&app, "/api/sync/heartbeat", &document, None).await;
        assert_eq!(status, 403, "a {who} reached the heartbeat: {body}");
    }

    let app = build_test_app(db.clone());
    let (status, _) = post(&app, "/api/sync/heartbeat", &document, None).await;
    assert_eq!(status, 401);
}

/// Identity binds from the credential, so naming a sibling in the body touches nothing.
#[tokio::test]
async fn test_heartbeat_cannot_reach_another_device() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code_a = seed_connect_code(&db, "alice").await;
    let code_b = seed_connect_code(&db, "bob").await;
    let token_a = enrol_device(&app, &code_a, "Alice laptop").await;
    enrol_device(&app, &code_b, "Bob laptop").await;

    let bob_id: String =
        one_value(&db, "SELECT id::text FROM device WHERE name = 'Bob laptop'").await;
    let (status, body) = post(
        &app,
        "/api/sync/heartbeat",
        &serde_json::json!({
            "id": bob_id, "device_id": bob_id, "gui_version": "9.9.9",
        }),
        Some(&token_a),
    )
    .await;
    assert_eq!(status, 204, "{body}");

    let bob_gui: Option<String> = one_value(
        &db,
        "SELECT gui_version FROM device WHERE name = 'Bob laptop'",
    )
    .await;
    assert_eq!(bob_gui, None, "a heartbeat wrote to a sibling");
    let alice_gui: Option<String> = one_value(
        &db,
        "SELECT gui_version FROM device WHERE name = 'Alice laptop'",
    )
    .await;
    assert_eq!(alice_gui.as_deref(), Some("9.9.9"));
}

#[tokio::test]
async fn test_heartbeat_rejects_a_scalar_system_profile() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    let (status, _) = post(
        &app,
        "/api/sync/heartbeat",
        &serde_json::json!({ "system_profile": "RTX 4070" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 400);
}
