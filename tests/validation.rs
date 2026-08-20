//! Input validation on the generated CRUD routes: what 422s, what passes, and the
//! error shape the console renders.

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

#[tokio::test]
async fn test_create_site_refuses_an_empty_name() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    for name in ["", "   ", "\t\n"] {
        let (status, body) = post_json(&app, "/api/sites", &site_body(name), None).await;
        assert_eq!(status, 422, "accepted {name:?}: {body}");
    }

    let rows: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(rows, 0, "a refused site was stored anyway");
}

#[tokio::test]
async fn test_validation_error_shape() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(&app, "/api/sites", &site_body(""), None).await;
    assert_eq!(status, 422);
    assert_eq!(
        body,
        serde_json::json!({
            "error": "Validation failed",
            "details": ["name: This field is required"],
        }),
        "the console renders this shape"
    );
}

#[tokio::test]
async fn test_create_site_bounds_coordinates() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    for (field, value) in [
        ("latitude", 90.0001),
        ("latitude", -90.0001),
        ("longitude", 180.0001),
        ("longitude", -180.0001),
    ] {
        let mut body = site_body("Harat");
        body[field] = serde_json::json!(value);
        let (status, response) = post_json(&app, "/api/sites", &body, None).await;
        assert_eq!(status, 422, "accepted {field}={value}: {response}");
    }

    // Exactly on the bound is a real place.
    let mut body = site_body("Poles");
    body["latitude"] = serde_json::json!(-90.0);
    body["longitude"] = serde_json::json!(180.0);
    let (status, response) = post_json(&app, "/api/sites", &body, None).await;
    assert_eq!(status, 201, "refused a boundary coordinate: {response}");
}

#[tokio::test]
async fn test_create_site_accepts_a_unicode_name() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(&app, "/api/sites", &site_body("Récif Ütopia 礁"), None).await;
    assert_eq!(status, 201, "{body}");
}

#[tokio::test]
async fn test_update_site_with_an_empty_body_changes_nothing() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (status, body) = put(
        &app,
        &format!("/api/sites/{id}"),
        &serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let name: String = one_value(&db, &format!("SELECT name FROM site WHERE id = '{id}'")).await;
    assert_eq!(name, "Harat");
}

#[tokio::test]
async fn test_update_site_refuses_a_whitespace_name() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (status, body) = put(
        &app,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "name": "  " }),
        None,
    )
    .await;
    assert_eq!(status, 422, "{body}");

    let name: String = one_value(&db, &format!("SELECT name FROM site WHERE id = '{id}'")).await;
    assert_eq!(name, "Harat", "a refused rename landed anyway");
}

#[tokio::test]
async fn test_update_site_bounds_coordinates() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (status, body) = put(
        &app,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "latitude": 91.0 }),
        None,
    )
    .await;
    assert_eq!(status, 422, "{body}");

    // Clearing a coordinate is not a coordinate, so no bound applies.
    let (status, body) = put(
        &app,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "latitude": null }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn test_batch_create_is_all_or_nothing_on_validation() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(
        &app,
        "/api/sites/batch",
        &serde_json::json!([site_body("Harat"), site_body(""), site_body("Fanous")]),
        None,
    )
    .await;
    assert_eq!(status, 422, "{body}");

    let rows: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(rows, 0, "a batch with an invalid row half landed");
}

#[tokio::test]
async fn test_batch_create_partial_keeps_the_valid_rows() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(
        &app,
        "/api/sites/batch?partial=true",
        &serde_json::json!([site_body("Harat"), site_body("")]),
        None,
    )
    .await;
    assert_eq!(status, 207, "{body}");

    let rows: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(rows, 1, "partial mode stored the wrong rows");
}

#[tokio::test]
async fn test_create_transect_bounds_end_points() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(
        &app,
        "/api/transects",
        &serde_json::json!({
            "name": "T1",
            "description": "",
            "start_lat": 27.0,
            "start_lon": 33.8,
            "end_lat": 27.0,
            "end_lon": 200.0,
        }),
        None,
    )
    .await;
    assert_eq!(status, 422, "accepted end_lon=200: {body}");
}

#[tokio::test]
async fn test_create_preset_refuses_version_zero() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) = post_json(
        &app,
        "/api/presets",
        &serde_json::json!({
            "name": "eceo-default",
            "version": 0,
            "settings": {},
            "description": "",
        }),
        None,
    )
    .await;
    assert_eq!(status, 422, "{body}");
}

/// A client that round-trips a whole record into an update is refused, never silently
/// trimmed: the console strips server-owned keys before it saves.
#[tokio::test]
async fn test_update_site_refuses_unknown_fields() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    for field in ["server_seq", "id", "no_such_column"] {
        let (status, body) = put(
            &app,
            &format!("/api/sites/{id}"),
            &serde_json::json!({ "description": "amended", field: "x" }),
            None,
        )
        .await;
        assert_eq!(status, 422, "accepted {field}: {body}");
    }

    let description: String = one_value(
        &db,
        &format!("SELECT description FROM site WHERE id = '{id}'"),
    )
    .await;
    assert_eq!(description, "", "a refused update landed anyway");
}

/// The sync path keeps its own DTOs, so refusing unknown CRUD fields must not touch a
/// device push that carries every server-stamped column.
#[tokio::test]
async fn test_push_still_accepts_server_stamped_columns() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &serde_json::json!({
            "contract_version": 1,
            "sections": { "transects": [{
                "id": "000000b1-0000-4000-8000-000000000000",
                "name": "T1",
                "description": "",
                "start_lat": 27.0, "start_lon": 33.8,
                "end_lat": 27.0, "end_lon": 33.9,
                "created_at": "2026-08-01T00:00:00Z",
                "updated_at": "2026-08-01T10:00:00Z",
                "deleted_at": null,
            }] },
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

/// Validation speaks before authorisation is relevant: a member may create, so an
/// invalid create answers 422 rather than anything about the role.
#[tokio::test]
async fn test_member_gets_422_not_403() {
    let db = setup_test_db().await;
    let app = build_test_app_as_member(db.clone());

    let (status, body) = post_json(&app, "/api/sites", &site_body(""), None).await;
    assert_eq!(status, 422, "{body}");
}
