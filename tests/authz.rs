//! Deletion as a tombstone, and who is allowed to write what.
//!
//! No Keycloak runs in the harness, so member and admin identities come from
//! `build_test_app_as_*`, which forces the identity the middleware would have resolved.

#[allow(dead_code)]
mod common;

use common::*;
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

fn site_body(name: &str) -> serde_json::Value {
    serde_json::json!({ "name": name, "description": "" })
}

fn video_body(file_name: &str) -> serde_json::Value {
    serde_json::json!({
        "file_name": file_name,
        "gravity": "unknown",
        "gps": "unknown",
    })
}

async fn create_site(app: &axum::Router, name: &str) -> String {
    let (status, body) = post_json(app, "/api/sites", &site_body(name), None).await;
    assert_eq!(status, 201, "site create failed: {body}");
    body["id"].as_str().expect("id in response").to_string()
}

/// `deleted_at`, `updated_at` and `server_seq` straight from Postgres, so the assertions
/// cannot be satisfied by whatever the API chooses to report.
async fn site_row(db: &DatabaseConnection, id: &str) -> Option<(Option<String>, String, i64)> {
    sync_row(db, "site", id).await
}

async fn sync_row(
    db: &DatabaseConnection,
    table: &str,
    id: &str,
) -> Option<(Option<String>, String, i64)> {
    let row = db
        .query_one_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT deleted_at::text AS deleted_at, updated_at::text AS updated_at, \
                 server_seq FROM {table} WHERE id = '{id}'"
            ),
        ))
        .await
        .expect("query runs")?;
    Some((
        row.try_get("", "deleted_at").ok(),
        row.try_get("", "updated_at").expect("updated_at"),
        row.try_get("", "server_seq").expect("server_seq"),
    ))
}

#[tokio::test]
async fn test_delete_site_tombstones_instead_of_removing() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (_, updated_before, seq_before) = site_row(&db, &id).await.expect("row exists");

    let (status, body) = delete(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204, "delete failed: {body}");

    let (deleted_at, updated_after, seq_after) =
        site_row(&db, &id).await.expect("row survives deletion");
    assert!(deleted_at.is_some(), "deleted_at was not stamped");
    assert_ne!(updated_after, updated_before, "updated_at did not move");
    assert!(
        seq_after > seq_before,
        "server_seq did not advance: {seq_before} then {seq_after}"
    );
}

#[tokio::test]
async fn test_delete_site_reaches_a_device_through_pull() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let device_app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Alice laptop").await;
    let token = enrol_device(&device_app, &code).await;

    let id = create_site(&admin, "Harat").await;
    let (_, pulled) = get_json(&device_app, "/api/sync/pull", Some(&token)).await;
    let cursor = pulled["cursor"].as_i64().expect("cursor");

    let (status, _) = delete(&admin, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204);

    let (status, pulled) = get_json(
        &device_app,
        &format!("/api/sync/pull?since={cursor}"),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200);
    let sites = pulled["sections"]["sites"]
        .as_array()
        .expect("sites section");
    assert_eq!(sites.len(), 1, "the tombstone did not pull: {pulled}");
    assert_eq!(sites[0]["id"], id);
    assert!(
        !sites[0]["deleted_at"].is_null(),
        "the pulled row carries no deleted_at: {pulled}"
    );
}

#[tokio::test]
async fn test_delete_site_is_idempotent() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (first, _) = delete(&app, &format!("/api/sites/{id}"), None).await;
    let (_, updated_after_first, _) = site_row(&db, &id).await.expect("row survives");
    let (second, body) = delete(&app, &format!("/api/sites/{id}"), None).await;

    assert_eq!(first, 204);
    assert_eq!(second, 204, "repeat delete: {body}");
    let (deleted_at, updated_after_second, _) = site_row(&db, &id).await.expect("row survives");
    assert!(deleted_at.is_some());
    // The first tombstone stands: a second delete must not restamp the clock a client
    // resolves conflicts on.
    assert_eq!(updated_after_second, updated_after_first);
}

#[tokio::test]
async fn test_delete_site_rejects_unknown_id() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, _) = delete(
        &app,
        "/api/sites/11111111-1111-4111-8111-111111111111",
        None,
    )
    .await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn test_delete_site_frees_the_unique_name() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (status, _) = delete(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204);

    let (status, body) = post_json(&app, "/api/sites", &site_body("Harat"), None).await;
    assert_eq!(status, 201, "the tombstone still holds the name: {body}");
    assert_ne!(body["id"].as_str().expect("id"), id);
}

#[tokio::test]
async fn test_delete_many_sites_tombstones_each() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let first = create_site(&app, "Harat").await;
    let second = create_site(&app, "Fanous").await;
    let (status, body) = delete_with_body(
        &app,
        "/api/sites/batch",
        Some(&serde_json::json!([first, second])),
        None,
    )
    .await;
    assert_eq!(status, 200, "batch delete failed: {body}");

    for id in [&first, &second] {
        let (deleted_at, _, _) = site_row(&db, id).await.expect("row survives");
        assert!(deleted_at.is_some(), "{id} was not tombstoned");
    }
}

#[tokio::test]
async fn test_delete_many_sites_reports_only_rows_that_existed() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let real = create_site(&app, "Harat").await;
    let phantom = "11111111-1111-4111-8111-111111111111";
    let (status, body) = delete_with_body(
        &app,
        "/api/sites/batch",
        Some(&serde_json::json!([real, phantom])),
        None,
    )
    .await;
    assert_eq!(status, 200, "batch delete failed: {body}");

    // The secure profile reports a count, never which ids existed.
    let reported: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    assert_eq!(
        reported,
        serde_json::json!({ "deleted": 1 }),
        "a phantom id was counted as deleted"
    );
    let (deleted_at, _, _) = site_row(&db, &real).await.expect("row survives");
    assert!(deleted_at.is_some(), "the real row was not tombstoned");
}

#[tokio::test]
async fn test_delete_many_sites_accepts_an_empty_batch() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let (status, body) =
        delete_with_body(&app, "/api/sites/batch", Some(&serde_json::json!([])), None).await;
    assert_eq!(status, 200, "{body}");
    let reported: serde_json::Value = serde_json::from_str(&body).expect("JSON body");
    assert_eq!(reported, serde_json::json!({ "deleted": 0 }));
}

#[tokio::test]
async fn test_delete_many_sites_refuses_an_oversized_batch() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let ids: Vec<String> = (0..101)
        .map(|n| format!("{n:0>8}-1111-4111-8111-111111111111"))
        .collect();
    let (status, body) = delete_with_body(
        &app,
        "/api/sites/batch",
        Some(&serde_json::json!(ids)),
        None,
    )
    .await;
    assert_eq!(status, 400, "an oversized batch was accepted: {body}");
}

#[tokio::test]
async fn test_list_sites_hides_tombstones() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    create_site(&app, "Fanous").await;
    let (status, _) = delete(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204);

    let (status, listed) = get_json(&app, "/api/sites", None).await;
    assert_eq!(status, 200);
    let sites = listed.as_array().expect("array of sites");
    assert_eq!(sites.len(), 1, "tombstone leaked into the list: {listed}");
    assert_eq!(sites[0]["name"], "Fanous");

    // Asking for tombstones explicitly must not get them either: `/api/sync/pull` is
    // the only route that hands them out.
    let (status, listed) = get_json(
        &app,
        "/api/sites?filter=%7B%22deleted_at%22%3A%22not_null%22%7D",
        None,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(
        listed.as_array().expect("array of sites").len(),
        1,
        "a caller-supplied deleted_at filter overrode the guard: {listed}"
    );
}

#[tokio::test]
async fn test_get_one_site_hides_a_tombstone() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());

    let id = create_site(&app, "Harat").await;
    let (status, _) = delete(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 204);

    let (status, body) = get(&app, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 404, "get-one served a tombstone: {body}");
}

#[tokio::test]
async fn test_update_site_refuses_a_member_supplied_deleted_at() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let member = build_test_app_as_member(db.clone());

    let id = create_site(&admin, "Harat").await;
    let (status, body) = put(
        &member,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "deleted_at": "2020-01-01T00:00:00Z" }),
        None,
    )
    .await;
    assert_eq!(status, 422, "a server-owned field was accepted: {body}");

    let (deleted_at, _, _) = site_row(&db, &id).await.expect("row exists");
    assert!(
        deleted_at.is_none(),
        "a member deleted through an edit, bypassing the delete guard: {deleted_at:?}"
    );
}

/// `updated_at` is the key last-write-wins resolves on, so a client that could set it
/// would pin a row against every later push.
#[tokio::test]
async fn test_update_site_stamps_updated_at_itself() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());

    let id = create_site(&admin, "Kanton").await;
    let (_, created_stamp, _) = site_row(&db, &id).await.expect("row exists");

    // Carrying the stamp is refused whole, so it cannot pin the row either way.
    let (status, body) = put(
        &admin,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "description": "amended", "updated_at": "2030-01-01T00:00:00Z" }),
        None,
    )
    .await;
    assert_eq!(status, 422, "a server-owned field was accepted: {body}");

    let (status, body) = put(
        &admin,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "description": "amended" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "admin edit refused: {body}");

    let (_, stamp, _) = site_row(&db, &id).await.expect("row exists");
    assert_ne!(
        stamp, created_stamp,
        "an edit left the conflict key stale, so a device's next push silently reverts it"
    );
}

#[tokio::test]
async fn test_create_site_refuses_a_client_supplied_provenance() {
    let db = setup_test_db().await;
    let app = build_test_app_as_member(db.clone());

    // The refusal is the deserialiser's, so the body is text naming the field.
    let (status, body) = post(
        &app,
        "/api/sites",
        &serde_json::json!({
            "name": "Harat",
            "description": "",
            "deleted_at": "2020-01-01T00:00:00Z",
            "device_id": "11111111-1111-4111-8111-111111111111",
        }),
        None,
    )
    .await;
    assert_eq!(status, 422, "a server-owned field was accepted: {body}");
    assert!(
        body.contains("unknown field"),
        "an unhelpful refusal: {body}"
    );

    let rows: i64 = one_value(&db, "SELECT COUNT(*)::BIGINT FROM site").await;
    assert_eq!(rows, 0, "the refused create landed anyway");
}

#[tokio::test]
async fn test_delete_site_rejects_a_member() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let member = build_test_app_as_member(db.clone());

    let id = create_site(&admin, "Harat").await;
    let (status, body) = delete(&member, &format!("/api/sites/{id}"), None).await;
    assert_eq!(status, 403);
    assert!(
        body.contains("deepreefmap-admin"),
        "the refusal says nothing about what is required: {body}"
    );
    let (deleted_at, _, _) = site_row(&db, &id).await.expect("row survives");
    assert!(deleted_at.is_none(), "a member tombstoned a site");
}

#[tokio::test]
async fn test_delete_site_rejects_a_device() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let device_app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Alice laptop").await;
    let token = enrol_device(&device_app, &code).await;

    let id = create_site(&admin, "Harat").await;
    let (status, body) = delete(&device_app, &format!("/api/sites/{id}"), Some(&token)).await;
    assert_eq!(status, 403);
    assert!(
        body.contains("/api/sync/push"),
        "the refusal does not point at sync push: {body}"
    );
    let (deleted_at, _, _) = site_row(&db, &id).await.expect("row survives");
    assert!(deleted_at.is_none(), "a device tombstoned a site");
}

#[tokio::test]
async fn test_create_site_allows_a_member() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());

    let (status, body) = post_json(&member, "/api/sites", &site_body("Harat"), None).await;
    assert_eq!(
        status, 201,
        "a member cannot set up survey metadata: {body}"
    );

    let id = body["id"].as_str().expect("id");
    let (status, body) = put(
        &member,
        &format!("/api/sites/{id}"),
        &serde_json::json!({ "name": "Harat", "description": "", "region": "Al Wajh" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "a member cannot edit a site: {body}");
}

#[tokio::test]
async fn test_mint_connect_code_allows_a_member() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());

    let (status, body) = post_json(
        &member,
        "/api/devices/connect-codes",
        &serde_json::json!({ "device_name": "my laptop" }),
        None,
    )
    .await;
    assert_eq!(
        status, 200,
        "a member cannot enrol their own laptop: {body}"
    );
}

#[tokio::test]
async fn test_device_reported_rows_are_read_only() {
    let db = setup_test_db().await;

    // Footage metadata, run provenance and cover measurements come from a device. The
    // registry records them, so no person writes them, admin included.
    for (who, app) in [
        ("member", build_test_app_as_member(db.clone())),
        ("admin", build_test_app_as_admin(db.clone())),
    ] {
        for resource in ["videos", "runs", "cover_rows"] {
            let (status, _) = post(
                &app,
                &format!("/api/{resource}"),
                &video_body("a.mp4"),
                None,
            )
            .await;
            assert_eq!(status, 405, "a {who} reached POST /api/{resource}");
            let (status, _) = post(
                &app,
                &format!("/api/{resource}/batch"),
                &serde_json::json!([]),
                None,
            )
            .await;
            assert_eq!(status, 405, "a {who} reached the {resource} batch route");
        }
    }
}

#[tokio::test]
async fn test_list_videos_allows_a_member() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());

    let (status, _) = get_json(&member, "/api/videos", None).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn test_push_allows_a_device() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Alice laptop").await;
    let token = enrol_device(&app, &code).await;

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &serde_json::json!({
            "contract_version": 1,
            "sections": {
                "transects": [{
                    "id": "11111111-1111-4111-8111-111111111111",
                    "name": "T1",
                    "description": "",
                    "start_lat": 12.0, "start_lon": 43.0,
                    "end_lat": 12.001, "end_lon": 43.001,
                    "created_at": "2026-08-01T00:00:00Z",
                    "updated_at": "2026-08-01T10:00:00Z",
                }],
                "videos": [{
                    "id": "22222222-2222-4222-8222-222222222222",
                    "file_name": "a.mp4",
                    "gravity": "unknown",
                    "gps": "unknown",
                    "created_at": "2026-08-01T00:00:00Z",
                    "updated_at": "2026-08-01T10:00:00Z",
                }],
            }
        }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "sync push is gated: {body}");
    assert_eq!(body["sections"]["transects"]["applied"], 1);
    assert_eq!(body["sections"]["videos"]["applied"], 1);
}

// --- The fixed device capability set ---

/// Enrol one laptop and hand back the router plus its token.
async fn device(db: &DatabaseConnection, minted_by: &str, name: &str) -> (axum::Router, String) {
    let app = build_test_app(db.clone());
    let code = seed_connect_code(db, minted_by, name).await;
    let token = enrol_device(&app, &code).await;
    (app, token)
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

fn pushed_site(id: &str, name: &str, updated_at: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": updated_at,
    })
}

/// A transect as a laptop sends it: the shallowest section a device actually authors, so
/// the ownership rules are exercised on a row the contract lets a device write.
fn pushed_transect(id: &str, name: &str, updated_at: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "name": name,
        "description": "",
        "start_lat": 12.0, "start_lon": 43.0,
        "end_lat": 12.001, "end_lon": 43.001,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": updated_at,
    })
}

#[tokio::test]
async fn test_crud_rejects_a_device_on_every_verb() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    let site = create_site(&admin, "Harat").await;
    let one = format!("/api/sites/{site}");

    // Reading is denied too: the console's surface is not a device's, in either direction.
    let refusals = vec![
        get(&app, "/api/sites", Some(&token)).await,
        get(&app, &one, Some(&token)).await,
        get(&app, "/api/devices", Some(&token)).await,
        get(&app, "/api/videos", Some(&token)).await,
        post(&app, "/api/sites", &site_body("Fanous"), Some(&token)).await,
        put(&app, &one, &site_body("Renamed"), Some(&token)).await,
        delete(&app, &one, Some(&token)).await,
        delete_with_body(
            &app,
            "/api/sites/batch",
            Some(&serde_json::json!([site])),
            Some(&token),
        )
        .await,
    ];

    for (status, body) in refusals {
        assert_eq!(status, 403, "a device reached a CRUD route: {body}");
        assert!(
            body.contains("/api/sync/push"),
            "the refusal does not name what a device may use: {body}"
        );
    }

    let (deleted_at, _, _) = site_row(&db, &site).await.expect("row survives");
    assert!(deleted_at.is_none(), "a device tombstoned a site");
    let (_, listed) = get_json(&admin, "/api/sites", None).await;
    assert_eq!(
        listed.as_array().expect("array of sites").len(),
        1,
        "a device created a site: {listed}"
    );
}

#[tokio::test]
async fn test_credential_routes_reject_a_device() {
    let db = setup_test_db().await;
    let (app, token) = device(&db, "alice", "Alice laptop").await;
    let device_id: String = one_value(&db, "SELECT id::text FROM device").await;

    let refusals = vec![
        post(
            &app,
            "/api/devices/connect-codes",
            &serde_json::json!({ "device_name": "another laptop" }),
            Some(&token),
        )
        .await,
        post(
            &app,
            &format!("/api/devices/{device_id}/revoke"),
            &serde_json::json!({}),
            Some(&token),
        )
        .await,
        post(
            &app,
            &format!("/api/devices/{device_id}/rename"),
            &serde_json::json!({ "name": "Something else" }),
            Some(&token),
        )
        .await,
    ];

    for (status, body) in refusals {
        assert_eq!(status, 403, "a device reached a credential route: {body}");
        assert!(
            body.contains("interactive login"),
            "the refusal does not say what is required: {body}"
        );
    }

    // Attribution stays what the operator set, and the token stays live.
    let name: String = one_value(&db, "SELECT name FROM device").await;
    assert_eq!(name, "Alice laptop");
    let revoked: Option<String> = one_value(&db, "SELECT revoked_at::text FROM device").await;
    assert_eq!(revoked, None);
}

#[tokio::test]
async fn test_sync_protocol_allows_a_device() {
    let db = setup_test_db().await;
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    for path in ["/api/sync/pull", "/api/me"] {
        let (status, body) = get(&app, path, Some(&token)).await;
        assert_eq!(status, 200, "{path} is gated against a device: {body}");
    }

    let (status, me) = get_json(&app, "/api/me", Some(&token)).await;
    assert_eq!(status, 200, "a device cannot read its own identity: {me}");

    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [pushed_transect(
                "11111111-1111-4111-8111-111111111111",
                "T1",
                "2026-08-01T10:00:00Z",
            )]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "sync push is gated: {body}");
    assert_eq!(body["sections"]["transects"]["applied"], 1);
}

// --- Origin-owned writes ---

#[tokio::test]
async fn test_push_updates_a_device_own_row() {
    let db = setup_test_db().await;
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    let id = "22222222-2222-4222-8222-222222222222";
    let (_, inserted) = post_json(
        &app,
        "/api/sync/push",
        &push_body(
            &serde_json::json!({ "transects": [pushed_transect(id, "T1", "2026-08-01T10:00:00Z")] }),
        ),
        Some(&token),
    )
    .await;
    assert_eq!(inserted["sections"]["transects"]["applied"], 1);

    let (status, updated) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [pushed_transect(id, "T1 North", "2026-08-01T11:00:00Z")]
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{updated}");
    assert_eq!(updated["sections"]["transects"]["applied"], 1);
    assert!(
        updated["sections"]["transects"]["refused"]
            .as_array()
            .expect("refused list")
            .is_empty(),
        "a laptop was refused its own row: {updated}"
    );

    let name: String =
        one_value(&db, &format!("SELECT name FROM transect WHERE id = '{id}'")).await;
    assert_eq!(name, "T1 North");
}

#[tokio::test]
async fn test_push_refuses_another_devices_row() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code_a = seed_connect_code(&db, "alice", "Alice laptop").await;
    let code_b = seed_connect_code(&db, "bob", "Bob laptop").await;
    let token_a = enrol_device(&app, &code_a).await;
    let token_b = enrol_device(&app, &code_b).await;

    let id = "33333333-3333-4333-8333-333333333333";
    post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [pushed_transect(id, "Alice's name", "2026-08-01T10:00:00Z")]
        })),
        Some(&token_a),
    )
    .await;

    // Newer, so last-write-wins alone would have taken it.
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "transects": [pushed_transect(id, "Bob's name", "2026-08-01T20:00:00Z")]
        })),
        Some(&token_b),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["transects"]["applied"], 0);
    assert_eq!(
        body["sections"]["transects"]["refused"][0], id,
        "the refusal was silent: {body}"
    );
    assert!(
        body["sections"]["transects"]["skipped"]
            .as_array()
            .expect("skipped list")
            .is_empty(),
        "an origin refusal was reported as a stale skip: {body}"
    );

    let name: String =
        one_value(&db, &format!("SELECT name FROM transect WHERE id = '{id}'")).await;
    assert_eq!(name, "Alice's name", "one laptop overwrote another's row");

    // A tombstone is a write like any other.
    let mut tombstone = pushed_transect(id, "Alice's name", "2026-08-01T21:00:00Z");
    tombstone["deleted_at"] = serde_json::json!("2026-08-01T21:00:00Z");
    let (_, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "transects": [tombstone] })),
        Some(&token_b),
    )
    .await;
    assert_eq!(body["sections"]["transects"]["refused"][0], id, "{body}");
    let (deleted_at, _, _) = sync_row(&db, "transect", id).await.expect("row survives");
    assert!(deleted_at.is_none(), "one laptop tombstoned another's row");
}

#[tokio::test]
async fn test_push_refuses_a_server_authored_row() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    let id = create_site(&admin, "Harat").await;
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "sites": [pushed_site(&id, "Renamed", &soon())] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["sections"]["sites"]["applied"], 0);
    assert_eq!(
        body["sections"]["sites"]["refused"][0], id,
        "a device amended a row authored on the server: {body}"
    );

    let name: String = one_value(&db, &format!("SELECT name FROM site WHERE id = '{id}'")).await;
    assert_eq!(name, "Harat");
}

/// A site or campaign made in the field lands unvalidated, for the console to check.
#[tokio::test]
async fn test_a_device_authors_a_site_and_a_campaign_unvalidated() {
    let db = setup_test_db().await;
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    let site = "66666666-6666-4666-8666-666666666666";
    let campaign = "77777777-7777-4777-8777-777777777777";
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "sites": [pushed_site(site, "New reef", "2026-08-01T10:00:00Z")],
            "campaigns": [{
                "id": campaign, "name": "2026_08_djibouti", "description": "",
                "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
            }],
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");
    for section in ["sites", "campaigns"] {
        assert_eq!(body["sections"][section]["applied"], 1, "{body}");
    }

    let validated: Option<String> = one_value(
        &db,
        &format!("SELECT validated_at::text FROM site WHERE id = '{site}'"),
    )
    .await;
    assert_eq!(validated, None, "a field-made site arrived validated");
    let campaigns: i64 = one_value(&db, "SELECT COUNT(*) FROM campaign").await;
    assert_eq!(campaigns, 1);
}

#[tokio::test]
async fn test_pull_restricts_a_device_to_downloadable_sections() {
    let db = setup_test_db().await;
    let (app, token) = device(&db, "alice", "Alice laptop").await;

    // The site comes from a human, since a device does not author one.
    create_site(&build_test_app_as_admin(db.clone()), "Harat").await;

    let (status, pushed) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({
            "videos": [{
                "id": "55555555-5555-4555-8555-555555555555",
                "file_name": "a.mp4", "gravity": "unknown", "gps": "unknown",
                "created_at": "2026-08-01T00:00:00Z", "updated_at": "2026-08-01T10:00:00Z",
            }],
        })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{pushed}");

    let (status, pulled) = get_json(&app, "/api/sync/pull", Some(&token)).await;
    assert_eq!(status, 200);
    let sections: Vec<&String> = pulled["sections"]
        .as_object()
        .expect("sections object")
        .keys()
        .collect();
    // The migration's seeded preset rides along; presets are pull only.
    assert_eq!(sections, vec!["presets", "sites"], "{pulled}");
    // Uploaded and never offered back, so the cursor must not claim there is more.
    assert_eq!(pulled["has_more"], false, "{pulled}");

    // An operator still sees the lot.
    let admin = build_test_app_as_admin(db.clone());
    let (_, pulled) = get_json(&admin, "/api/sync/pull", None).await;
    assert!(
        pulled["sections"]["videos"].is_array(),
        "an operator's pull lost the upload-only sections: {pulled}"
    );
}
