//! The depth a transect is filed under: derived from the two ends where both were
//! measured, and the historical single reading where they were not.

#[allow(dead_code)]
mod common;

use common::*;

const SITE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

async fn depth(db: &sea_orm::DatabaseConnection, id: &str, column: &str) -> Option<f64> {
    one_value(
        db,
        &format!("SELECT {column} FROM transect WHERE id = '{id}'"),
    )
    .await
}

async fn create(app: &axum::Router, body: &serde_json::Value) -> serde_json::Value {
    let (status, body) = post(app, "/api/transects", body, None).await;
    assert_eq!(status, 201, "{body}");
    serde_json::from_str(&body).expect("JSON")
}

#[tokio::test]
async fn test_depth_is_the_mean_of_the_two_ends() {
    let db = setup_test_db().await;
    seed_site(&db, SITE, "Japanese Garden").await;
    let app = build_test_app_as_admin(db.clone());

    let created = create(
        &app,
        &serde_json::json!({
            "site_id": SITE,
            "name": "T1",
            "description": "",
            "start_depth_m": 5.0,
            "end_depth_m": 11.0,
        }),
    )
    .await;

    let id = created["id"].as_str().expect("an id");
    assert_eq!(depth(&db, id, "depth_m").await, Some(8.0));
}

#[tokio::test]
async fn test_a_depth_sent_against_the_ends_does_not_stand() {
    let db = setup_test_db().await;
    seed_site(&db, SITE, "Japanese Garden").await;
    let app = build_test_app_as_admin(db.clone());

    let created = create(
        &app,
        &serde_json::json!({
            "site_id": SITE,
            "name": "T1",
            "description": "",
            "depth_m": 40.0,
            "start_depth_m": 5.0,
            "end_depth_m": 11.0,
        }),
    )
    .await;
    let id = created["id"].as_str().expect("an id").to_string();
    assert_eq!(depth(&db, &id, "depth_m").await, Some(8.0));

    let (status, body) = put(
        &app,
        &format!("/api/transects/{id}"),
        &serde_json::json!({ "end_depth_m": 15.0 }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(depth(&db, &id, "depth_m").await, Some(10.0));
}

#[tokio::test]
async fn test_a_single_historical_reading_survives_with_no_ends() {
    let db = setup_test_db().await;
    seed_site(&db, SITE, "Japanese Garden").await;
    let app = build_test_app_as_admin(db.clone());

    let created = create(
        &app,
        &serde_json::json!({
            "site_id": SITE,
            "name": "T1",
            "description": "",
            "depth_m": 8.5,
        }),
    )
    .await;

    let id = created["id"].as_str().expect("an id");
    assert_eq!(depth(&db, id, "depth_m").await, Some(8.5));
    assert_eq!(depth(&db, id, "start_depth_m").await, None);
}

#[tokio::test]
async fn test_one_end_alone_leaves_the_depth_where_it_was() {
    let db = setup_test_db().await;
    seed_site(&db, SITE, "Japanese Garden").await;
    let app = build_test_app_as_admin(db.clone());

    let created = create(
        &app,
        &serde_json::json!({
            "site_id": SITE,
            "name": "T1",
            "description": "",
            "depth_m": 8.5,
            "start_depth_m": 5.0,
        }),
    )
    .await;

    let id = created["id"].as_str().expect("an id");
    assert_eq!(depth(&db, id, "depth_m").await, Some(8.5));
}
