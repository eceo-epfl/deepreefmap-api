//! Survey events: console-curated pass groups, and who may touch them.

#[allow(dead_code)]
mod common;

use common::*;

fn group_body(name: &str, period_label: &str) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "period_label": period_label,
        "description": "",
    })
}

async fn create_group(app: &axum::Router, name: &str, period_label: &str) -> String {
    let (status, body) = post_json(
        app,
        "/api/pass_groups",
        &group_body(name, period_label),
        None,
    )
    .await;
    assert_eq!(status, 201, "group create failed: {body}");
    body["id"].as_str().expect("id in response").to_string()
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

fn pass_row(id: &str, updated_at: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "begin_s": 0.0,
        "end_s": 120.0,
        "upside_down": false,
        "label": "morning swim",
        "notes": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": updated_at,
    })
}

#[tokio::test]
async fn test_pass_group_crud_and_soft_delete() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let member = build_test_app_as_member(db.clone());

    let id = create_group(&admin, "2024 spring", "2024-04").await;

    let (status, listed) = get_json(&admin, "/api/pass_groups", None).await;
    assert_eq!(status, 200);
    assert_eq!(listed.as_array().expect("array").len(), 1);

    // Members curate too: grouping is analysis, not deletion.
    let (status, body) = put(
        &member,
        &format!("/api/pass_groups/{id}"),
        &serde_json::json!({ "name": "2024 spring survey" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "member edit refused: {body}");

    let (status, _) = delete(&member, &format!("/api/pass_groups/{id}"), None).await;
    assert_eq!(status, 403, "a member deleted a group");

    let (status, body) = delete(&admin, &format!("/api/pass_groups/{id}"), None).await;
    assert_eq!(status, 204, "delete failed: {body}");

    // A tombstone, not a removal, and hidden from every read.
    let deleted_at: Option<String> = one_value(
        &db,
        &format!("SELECT deleted_at::text FROM pass_group WHERE id = '{id}'"),
    )
    .await;
    assert!(deleted_at.is_some(), "the group was hard-deleted");
    let (status, _) = get(&admin, &format!("/api/pass_groups/{id}"), None).await;
    assert_eq!(status, 404, "get-one served a tombstone");
    let (_, listed) = get_json(&admin, "/api/pass_groups", None).await;
    assert!(listed.as_array().expect("array").is_empty());

    // The unique index covers live rows only, so the name frees up.
    create_group(&admin, "2024 spring survey", "2024-04").await;
}

#[tokio::test]
async fn test_pass_group_rejects_a_device() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Alice laptop").await;

    let id = create_group(&admin, "2024 spring", "2024-04").await;
    let one = format!("/api/pass_groups/{id}");

    let refusals = vec![
        get(&app, "/api/pass_groups", Some(&token)).await,
        get(&app, &one, Some(&token)).await,
        post(
            &app,
            "/api/pass_groups",
            &group_body("Invented", ""),
            Some(&token),
        )
        .await,
        put(&app, &one, &group_body("Renamed", ""), Some(&token)).await,
        delete(&app, &one, Some(&token)).await,
    ];
    for (status, body) in refusals {
        assert_eq!(status, 403, "a device reached a pass_group route: {body}");
        assert!(
            body.contains("/api/sync/push"),
            "the refusal does not name what a device may use: {body}"
        );
    }

    let groups: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM pass_group").await;
    assert_eq!(groups, 1, "a device wrote a group");
}

/// The safety property the column's absence from the sync contract buys: a device
/// re-pushing its own pass must not clear the grouping a curator assigned meanwhile.
#[tokio::test]
async fn test_survey_group_survives_a_device_repush() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Alice laptop").await;

    let pass = "11111111-1111-4111-8111-111111111111";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "passes": [pass_row(pass, "2026-08-01T10:00:00Z")] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["passes"]["applied"], 1, "{body}");

    let group = create_group(&admin, "2024 spring", "2024-04").await;
    let (status, body) = put(
        &admin,
        &format!("/api/passes/{pass}"),
        &serde_json::json!({ "survey_group_id": group }),
        None,
    )
    .await;
    assert_eq!(status, 200, "assigning the group failed: {body}");

    // Newer than the curator's edit, so last-write-wins takes the whole row again.
    let mut repushed = pass_row(pass, &soon());
    repushed["label"] = serde_json::json!("renamed swim");
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "passes": [repushed] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(
        body["sections"]["passes"]["applied"], 1,
        "the re-push was skipped, so nothing was exercised: {body}"
    );

    let label: String = one_value(
        &db,
        &format!("SELECT label FROM transect_pass WHERE id = '{pass}'"),
    )
    .await;
    assert_eq!(label, "renamed swim", "the re-push did not land");
    let stored: Option<String> = one_value(
        &db,
        &format!("SELECT survey_group_id::text FROM transect_pass WHERE id = '{pass}'"),
    )
    .await;
    assert_eq!(
        stored,
        Some(group),
        "a device re-push clobbered the curator's grouping"
    );
}
