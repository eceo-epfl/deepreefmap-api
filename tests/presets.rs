//! Server-defined presets: console CRUD, and the device-facing surface being pull only.

#[allow(dead_code)]
mod common;

use common::*;

fn preset_body(name: &str, version: i32, settings: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "version": version,
        "settings": settings,
        "description": "",
    })
}

async fn create_preset(
    app: &axum::Router,
    name: &str,
    version: i32,
    settings: &serde_json::Value,
) -> String {
    let (status, body) = post_json(
        app,
        "/api/presets",
        &preset_body(name, version, settings),
        None,
    )
    .await;
    assert_eq!(status, 201, "preset create failed: {body}");
    body["id"].as_str().expect("id in response").to_string()
}

#[tokio::test]
async fn test_preset_crud_and_soft_delete() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let member = build_test_app_as_member(db.clone());

    let settings = serde_json::json!({ "fps": 4, "mapping_backend": "loger" });
    let id = create_preset(&member, "eceo-default", 1, &settings).await;

    // The settings document survives storage byte-meaning intact.
    let (status, fetched) = get_json(&admin, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 200);
    assert_eq!(fetched["settings"], settings, "{fetched}");
    assert_eq!(fetched["version"], 1);

    let amended = serde_json::json!({ "fps": 8, "mapping_backend": "loger_star" });
    let (status, body) = put(
        &member,
        &format!("/api/presets/{id}"),
        &serde_json::json!({ "settings": amended }),
        None,
    )
    .await;
    assert_eq!(status, 200, "member edit refused: {body}");
    let (_, fetched) = get_json(&admin, &format!("/api/presets/{id}"), None).await;
    assert_eq!(fetched["settings"], amended, "{fetched}");

    let (status, _) = delete(&member, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 403, "a member deleted a preset");

    let (status, body) = delete(&admin, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 204, "delete failed: {body}");
    let deleted_at: Option<String> = one_value(
        &db,
        &format!("SELECT deleted_at::text FROM preset WHERE id = '{id}'"),
    )
    .await;
    assert!(deleted_at.is_some(), "the preset was hard-deleted");
    let (status, _) = get(&admin, &format!("/api/presets/{id}"), None).await;
    assert_eq!(status, 404, "get-one served a tombstone");

    // The unique index covers live rows only, so (name, version) frees up.
    create_preset(&admin, "eceo-default", 1, &settings).await;
}

#[tokio::test]
async fn test_preset_crud_rejects_a_device() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Alice laptop").await;

    let settings = serde_json::json!({ "fps": 4 });
    let id = create_preset(&admin, "eceo-default", 1, &settings).await;
    let one = format!("/api/presets/{id}");

    // Reading included: a device gets presets through sync pull, not the console's routes.
    let refusals = vec![
        get(&app, "/api/presets", Some(&token)).await,
        get(&app, &one, Some(&token)).await,
        post(
            &app,
            "/api/presets",
            &preset_body("invented", 1, &settings),
            Some(&token),
        )
        .await,
        put(
            &app,
            &one,
            &serde_json::json!({ "version": 9 }),
            Some(&token),
        )
        .await,
        delete(&app, &one, Some(&token)).await,
    ];
    for (status, body) in refusals {
        assert_eq!(status, 403, "a device reached a preset route: {body}");
        assert!(
            body.contains("/api/sync/push"),
            "the refusal does not name what a device may use: {body}"
        );
    }

    let presets: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM preset").await;
    assert_eq!(presets, 1, "a device wrote a preset");
}
