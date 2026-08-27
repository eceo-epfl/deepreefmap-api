//! The change ledger under the current contract: field-level merge, the console's
//! last word, proposals, and what a device pulls back about its own rows.

#[allow(dead_code)]
mod common;

use common::*;

use deepreefmap_api::common::contract::CONTRACT_HEADER;
use deepreefmap_api::routes::private::sync::schema::{CONTRACT_VERSION, MIN_CONTRACT_VERSION};

fn current() -> Vec<(String, String)> {
    vec![(
        CONTRACT_HEADER.to_string(),
        format!("{MIN_CONTRACT_VERSION}-{CONTRACT_VERSION}"),
    )]
}

async fn push(app: &axum::Router, token: &str, sections: serde_json::Value) -> serde_json::Value {
    let body = serde_json::json!({ "contract_version": CONTRACT_VERSION, "sections": sections });
    let (status, _, body) =
        post_declaring(app, "/api/sync/push", &body, Some(token), &current()).await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).expect("JSON")
}

async fn pull(app: &axum::Router, token: &str, since: i64) -> serde_json::Value {
    let (status, _, body) = get_declaring(
        app,
        &format!("/api/sync/pull?since={since}"),
        Some(token),
        &current(),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    serde_json::from_str(&body).expect("JSON")
}

const SITE: &str = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";

fn transect(id: &str, name: &str, base_seq: Option<i64>) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "site_id": SITE,
        "name": name,
        "description": "",
        "start_lat": 12.0, "start_lon": 43.0,
        "end_lat": 12.001, "end_lon": 43.001,
        "length_m": 100.0,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
        "base_seq": base_seq,
    })
}

fn ack_seq(outcome: &serde_json::Value, id: &str) -> i64 {
    outcome["applied"]
        .as_array()
        .unwrap_or_else(|| panic!("no acks: {outcome}"))
        .iter()
        .find(|ack| ack["id"] == id)
        .unwrap_or_else(|| panic!("{id} not applied: {outcome}"))["seq"]
        .as_i64()
        .expect("seq")
}

fn refusal<'a>(outcome: &'a serde_json::Value, bucket: &str, id: &str) -> &'a serde_json::Value {
    outcome[bucket]
        .as_array()
        .unwrap_or_else(|| panic!("no {bucket}: {outcome}"))
        .iter()
        .find(|r| r["id"] == id)
        .unwrap_or_else(|| panic!("{id} not {bucket}: {outcome}"))
}

async fn create_site(app: &axum::Router, name: &str) -> String {
    let body = serde_json::json!({ "name": name, "description": "" });
    let (status, body) = post_json(app, "/api/sites", &body, None).await;
    assert_eq!(status, 201, "site create failed: {body}");
    body["id"].as_str().expect("id").to_string()
}

async fn one_device(db: &sea_orm::DatabaseConnection) -> (axum::Router, String) {
    seed_site(db, SITE, "Harat").await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(db, "alice", "Alice laptop").await;
    let token = enrol_device(&app, &code).await;
    (app, token)
}

async fn name_of(db: &sea_orm::DatabaseConnection, id: &str) -> String {
    one_value(db, &format!("SELECT name FROM transect WHERE id = '{id}'")).await
}

const T1: &str = "11111111-1111-4111-8111-111111111111";

#[tokio::test]
async fn test_new_row_acks_with_the_row_position() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let seq = ack_seq(&pushed["sections"]["transects"], T1);
    let stored: i64 = one_value(
        &db,
        &format!("SELECT server_seq FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert_eq!(seq, stored);

    let status: String = one_value(&db, "SELECT status FROM change_log").await;
    assert_eq!(status, "applied");
}

#[tokio::test]
async fn test_disjoint_edits_from_console_and_device_both_land() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);

    let (status, body) = put(
        &admin,
        &format!("/api/transects/{T1}"),
        &serde_json::json!({ "description": "curated" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1 north", Some(base))] }),
    )
    .await;
    ack_seq(&pushed["sections"]["transects"], T1);

    assert_eq!(name_of(&db, T1).await, "T1 north");
    let description: String = one_value(
        &db,
        &format!("SELECT description FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert_eq!(
        description, "curated",
        "the merge dropped the console's field"
    );
}

#[tokio::test]
async fn test_depth_at_each_end_merges_field_by_field() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    let mut row = transect(T1, "T1", None);
    row["start_depth_m"] = serde_json::json!(5.0);
    let pushed = push(&app, &token, serde_json::json!({ "transects": [row] })).await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);

    let (status, body) = put(
        &admin,
        &format!("/api/transects/{T1}"),
        &serde_json::json!({ "end_depth_m": 11.0 }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let mut row = transect(T1, "T1", Some(base));
    row["start_depth_m"] = serde_json::json!(5.0);
    row["depth_m"] = serde_json::json!(8.0);
    let pushed = push(&app, &token, serde_json::json!({ "transects": [row] })).await;
    ack_seq(&pushed["sections"]["transects"], T1);

    let depth = |column: &'static str| {
        let db = db.clone();
        async move {
            let value: f64 = one_value(
                &db,
                &format!("SELECT {column} FROM transect WHERE id = '{T1}'"),
            )
            .await;
            value
        }
    };
    assert!((depth("start_depth_m").await - 5.0).abs() < f64::EPSILON);
    assert!(
        (depth("end_depth_m").await - 11.0).abs() < f64::EPSILON,
        "the merge dropped the console's end depth"
    );
    assert!((depth("depth_m").await - 8.0).abs() < f64::EPSILON); // (5 + 11) / 2
}

#[tokio::test]
async fn test_console_edit_makes_an_overlapping_device_edit_a_proposal() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);

    let (status, _) = put(
        &admin,
        &format!("/api/transects/{T1}"),
        &serde_json::json!({ "name": "Console name" }),
        None,
    )
    .await;
    assert_eq!(status, 200);

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "Device name", Some(base))] }),
    )
    .await;
    let proposal = refusal(&pushed["sections"]["transects"], "proposed", T1);
    assert_eq!(proposal["reason"], "console_edited");
    assert_eq!(name_of(&db, T1).await, "Console name");

    let (status, listed) = get_json(
        &admin,
        "/api/changes?filter=%7B%22status%22%3A%22proposed%22%7D",
        None,
    )
    .await;
    assert_eq!(status, 200, "{listed}");
    let entries = listed.as_array().expect("a list");
    assert_eq!(entries.len(), 1, "{listed}");
    assert_eq!(entries[0]["patch"]["name"], "Device name");
    let seq = entries[0]["seq"].as_i64().expect("seq");

    let (status, body) = post_json(
        &admin,
        &format!("/api/changes/{seq}/accept"),
        &serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(name_of(&db, T1).await, "Device name");
    let validated: Option<String> = one_value(
        &db,
        &format!("SELECT validated_at::text FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert!(validated.is_some(), "accepting did not validate the row");

    // Accepted twice is a conflict, not a second write.
    let (status, _) = post_json(
        &admin,
        &format!("/api/changes/{seq}/accept"),
        &serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, 409);
}

#[tokio::test]
async fn test_validated_row_takes_only_proposals_and_a_dismissal_reaches_the_device() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);
    let cursor = pushed["cursor"].as_i64().expect("cursor");

    let (status, body) = post_json(
        &admin,
        "/api/changes/validate",
        &serde_json::json!({ "section": "transects", "ids": [T1] }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["validated"][0], T1);

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "Renamed", Some(base))] }),
    )
    .await;
    let proposal = refusal(&pushed["sections"]["transects"], "proposed", T1);
    assert_eq!(proposal["reason"], "validated");
    assert_eq!(name_of(&db, T1).await, "T1");

    let seq: i64 = one_value(&db, "SELECT seq FROM change_log WHERE status = 'proposed'").await;
    let (status, _) = post_json(
        &admin,
        &format!("/api/changes/{seq}/dismiss"),
        &serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, 200);

    let pulled = pull(&app, &token, cursor).await;
    let outbox = pulled["outbox"].as_array().expect("outbox");
    assert_eq!(outbox.len(), 1, "{pulled}");
    assert_eq!(outbox[0]["status"], "dismissed");
    assert_eq!(outbox[0]["row_id"], T1);
    // The validated row itself comes down too, stamped.
    assert!(
        !pulled["sections"]["transects"][0]["validated_at"].is_null(),
        "{pulled}"
    );
}

#[tokio::test]
async fn test_validating_a_transect_without_a_site_is_refused() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());
    let mut row = transect(T1, "T1", None);
    row["site_id"] = serde_json::Value::Null;
    push(&app, &token, serde_json::json!({ "transects": [row] })).await;

    let (status, body) = post_json(
        &admin,
        "/api/changes/validate",
        &serde_json::json!({ "section": "transects", "ids": [T1] }),
        None,
    )
    .await;
    assert_eq!(status, 400, "{body}");
}

#[tokio::test]
async fn test_a_console_tombstone_is_not_resurrected() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);
    let (status, _) = delete(&admin, &format!("/api/transects/{T1}"), None).await;
    assert_eq!(status, 204);

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "Back again", Some(base))] }),
    )
    .await;
    let proposal = refusal(&pushed["sections"]["transects"], "proposed", T1);
    assert_eq!(proposal["reason"], "deleted");
    let deleted: Option<String> = one_value(
        &db,
        &format!("SELECT deleted_at::text FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert!(deleted.is_some(), "the tombstone was cleared");
}

#[tokio::test]
async fn test_another_devices_row_is_superseded() {
    let db = setup_test_db().await;
    seed_site(&db, SITE, "Harat").await;
    let app = build_test_app(db.clone());
    let code_a = seed_connect_code(&db, "alice", "Alice laptop").await;
    let code_b = seed_connect_code(&db, "bob", "Bob laptop").await;
    let token_a = enrol_device(&app, &code_a).await;
    let token_b = enrol_device(&app, &code_b).await;

    let pushed = push(
        &app,
        &token_a,
        serde_json::json!({ "transects": [transect(T1, "Alice", None)] }),
    )
    .await;
    let base = ack_seq(&pushed["sections"]["transects"], T1);
    let pushed = push(
        &app,
        &token_b,
        serde_json::json!({ "transects": [transect(T1, "Bob", Some(base))] }),
    )
    .await;
    let refused = refusal(&pushed["sections"]["transects"], "superseded", T1);
    assert_eq!(refused["reason"], "another_device");
    assert_eq!(name_of(&db, T1).await, "Alice");
}

#[tokio::test]
async fn test_a_stale_base_on_the_same_field_is_superseded() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "First", None)] }),
    )
    .await;
    let first = ack_seq(&pushed["sections"]["transects"], T1);
    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "Second", Some(first))] }),
    )
    .await;
    ack_seq(&pushed["sections"]["transects"], T1);

    // Edited against the first position, on the field the second edit moved.
    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "Third", Some(first))] }),
    )
    .await;
    let refused = refusal(&pushed["sections"]["transects"], "superseded", T1);
    assert_eq!(refused["reason"], "stale");
    assert_eq!(name_of(&db, T1).await, "Second");

    // Against the same stale base, another field merges over the newer name.
    let mut row = transect(T1, "First", Some(first));
    row["depth_m"] = serde_json::json!(8.5);
    let pushed = push(&app, &token, serde_json::json!({ "transects": [row] })).await;
    ack_seq(&pushed["sections"]["transects"], T1);
    let depth: f64 = one_value(
        &db,
        &format!("SELECT depth_m FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert!((depth - 8.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn test_a_rejected_row_is_recorded_with_its_reason() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;

    let pushed = push(
        &app,
        &token,
        serde_json::json!({ "passes": [{
            "id": T1, "begin_s": 0.0, "end_s": 10.0, "direction": "sideways",
            "upside_down": false, "label": "", "notes": "",
            "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
        }] }),
    )
    .await;
    let rejected = refusal(&pushed["sections"]["passes"], "rejected", T1);
    assert_eq!(rejected["reason"], "outside_vocabulary");
    let status: String = one_value(&db, "SELECT status FROM change_log").await;
    assert_eq!(status, "rejected");
}

#[tokio::test]
async fn test_a_push_without_a_base_cannot_touch_a_validated_row_or_its_stamp() {
    let db = setup_test_db().await;
    let (app, token) = one_device(&db).await;
    let admin = build_test_app_as_admin(db.clone());

    push(
        &app,
        &token,
        serde_json::json!({ "transects": [transect(T1, "T1", None)] }),
    )
    .await;
    let (status, _) = post_json(
        &admin,
        "/api/changes/validate",
        &serde_json::json!({ "section": "transects", "ids": [T1] }),
        None,
    )
    .await;
    assert_eq!(status, 200);

    // No base_seq: a row that never learnt of the stamp.
    let mut row = transect(T1, "Renamed", None);
    row.as_object_mut().expect("object").remove("base_seq");
    row["updated_at"] = serde_json::json!(soon());
    let pushed = push(&app, &token, serde_json::json!({ "transects": [row] })).await;
    assert_eq!(
        refused(&pushed["sections"]["transects"], "proposed")[0],
        T1,
        "{pushed}"
    );
    assert_eq!(name_of(&db, T1).await, "T1");
    let validated: Option<String> = one_value(
        &db,
        &format!("SELECT validated_at::text FROM transect WHERE id = '{T1}'"),
    )
    .await;
    assert!(validated.is_some());
}

#[tokio::test]
async fn test_own_rows_come_down_to_their_device_only() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code_a = seed_connect_code(&db, "alice", "Alice laptop").await;
    let code_b = seed_connect_code(&db, "bob", "Bob laptop").await;
    let token_a = enrol_device(&app, &code_a).await;
    let token_b = enrol_device(&app, &code_b).await;
    let admin = build_test_app_as_admin(db.clone());

    let pass = "22222222-2222-4222-8222-222222222222";
    let pushed = push(
        &app,
        &token_a,
        serde_json::json!({ "passes": [{
            "id": pass, "begin_s": 0.0, "end_s": 10.0, "direction": "forward",
            "upside_down": false, "label": "", "notes": "",
            "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
        }] }),
    )
    .await;
    ack_seq(&pushed["sections"]["passes"], pass);

    let (status, _) = delete(&admin, &format!("/api/passes/{pass}"), None).await;
    assert_eq!(status, 204);

    let mine = pull(&app, &token_a, 0).await;
    let passes = mine["sections"]["passes"]
        .as_array()
        .expect("own passes come down");
    assert_eq!(passes.len(), 1, "{mine}");
    assert!(
        !passes[0]["deleted_at"].is_null(),
        "the console's tombstone did not reach the device"
    );
    assert_eq!(mine["has_more"], false);

    let theirs = pull(&app, &token_b, 0).await;
    assert!(
        theirs["sections"]["passes"].is_null(),
        "another device's pass leaked: {theirs}"
    );
}

#[tokio::test]
async fn test_every_console_write_is_in_the_ledger() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());

    let id = create_site(&admin, "Harat").await;
    let (status, _) = put(
        &admin,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "country": "Eritrea" }),
        None,
    )
    .await;
    assert_eq!(status, 200);
    let (status, _) = delete(&admin, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204);

    let entries: i64 = one_value(
        &db,
        &format!("SELECT COUNT(*)::BIGINT FROM change_log WHERE table_key = 'sites' AND row_id = '{id}' AND author = 'admin-sub' AND status = 'applied'"),
    )
    .await;
    assert_eq!(entries, 3);
    let patched: String = one_value(
        &db,
        &format!(
            "SELECT patch->>'country' FROM change_log WHERE row_id = '{id}' AND patch ? 'country'"
        ),
    )
    .await;
    assert_eq!(patched, "Eritrea");
}
