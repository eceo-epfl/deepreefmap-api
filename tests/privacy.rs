//! Subject erasure and connect-code retention.

#[allow(dead_code)]
mod common;

use common::*;

#[tokio::test]
async fn test_erase_subject_scrubs_the_audit_columns() {
    let db = setup_test_db().await;
    let device_app = build_test_app(db.clone());
    let alice_code = seed_connect_code(&db, "alice", "Alice laptop").await;
    enrol_device(&device_app, &alice_code).await;
    let bob_code = seed_connect_code(&db, "bob", "Bob laptop").await;
    enrol_device(&device_app, &bob_code).await;
    exec(
        &db,
        "INSERT INTO stored_object (id, content_hash, size_bytes, kind, status, s3_key, uploaded_by) \
         VALUES (gen_random_uuid(), 'abc', 1, 'video', 'pending', 'dev/videos/imohash/abc', \
         'alice')",
    )
    .await;

    let app = build_test_app_as_admin(db.clone());
    let (status, body) = post_json(
        &app,
        "/api/admin/erase-subject",
        &serde_json::json!({ "subject": "alice" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["scrubbed"]["device"], 1);
    assert_eq!(body["scrubbed"]["connect_code"], 1);
    assert_eq!(body["scrubbed"]["stored_object"], 1);
    // Only the audit tables answer: survey rows attribute to a device, not a person,
    // so they hold no subject to scrub.
    assert_eq!(body["scrubbed"].as_object().unwrap().len(), 3);

    let enrolled_by: Option<String> = one_value(
        &db,
        "SELECT enrolled_by FROM device WHERE name = 'Alice laptop'",
    )
    .await;
    assert_eq!(enrolled_by, None);
    let bob: Option<String> = one_value(
        &db,
        "SELECT enrolled_by FROM device WHERE name = 'Bob laptop'",
    )
    .await;
    assert_eq!(bob.as_deref(), Some("bob"), "another subject was scrubbed");
    let minted_by: Option<String> = one_value(
        &db,
        "SELECT created_by FROM connect_code WHERE created_by IS NOT NULL",
    )
    .await;
    assert_eq!(minted_by.as_deref(), Some("bob"));
    let uploaded_by: Option<String> = one_value(&db, "SELECT uploaded_by FROM stored_object").await;
    assert_eq!(uploaded_by, None);
}

#[tokio::test]
async fn test_erase_subject_is_admin_only() {
    let db = setup_test_db().await;
    let body = serde_json::json!({ "subject": "alice" });

    let app = build_test_app_as_member(db.clone());
    let (status, answer) = post(&app, "/api/admin/erase-subject", &body, None).await;
    assert_eq!(status, 403, "{answer}");

    let device_app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&device_app, &code).await;
    let (status, _) = post(&device_app, "/api/admin/erase-subject", &body, Some(&token)).await;
    assert_eq!(status, 403);
    let (status, _) = post(&device_app, "/api/admin/erase-subject", &body, None).await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn test_erase_subject_rejects_an_empty_subject() {
    let db = setup_test_db().await;
    let app = build_test_app_as_admin(db.clone());
    let (status, _) = post(
        &app,
        "/api/admin/erase-subject",
        &serde_json::json!({ "subject": "  " }),
        None,
    )
    .await;
    assert_eq!(status, 400);
}

async fn seed_code_aged(db: &sea_orm::DatabaseConnection, name: &str, used: &str, expires: &str) {
    exec(
        db,
        &format!(
            "INSERT INTO connect_code \
             (id, code_hash, created_by, device_name, expires_at, used_at, created_at) \
             VALUES (gen_random_uuid(), '{name}', 'alice', '{name}', {expires}, {used}, NOW())"
        ),
    )
    .await;
}

/// Minting housekeeps: codes used or expired more than 90 days ago are hard deleted.
#[tokio::test]
async fn test_minting_deletes_long_dead_connect_codes() {
    let db = setup_test_db().await;
    seed_code_aged(
        &db,
        "old-used",
        "NOW() - INTERVAL '100 days'",
        "NOW() - INTERVAL '100 days'",
    )
    .await;
    seed_code_aged(&db, "old-expired", "NULL", "NOW() - INTERVAL '100 days'").await;
    seed_code_aged(
        &db,
        "recently-used",
        "NOW() - INTERVAL '1 day'",
        "NOW() - INTERVAL '1 day'",
    )
    .await;
    seed_code_aged(&db, "live", "NULL", "NOW() + INTERVAL '1 hour'").await;

    let app = build_test_app_as_admin(db.clone());
    let (status, body) = post(
        &app,
        "/api/devices/connect-codes",
        &serde_json::json!({ "device_name": "fresh" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let kept: Vec<String> = {
        let names: String = one_value(
            &db,
            "SELECT STRING_AGG(device_name, ',' ORDER BY device_name) FROM connect_code",
        )
        .await;
        names.split(',').map(String::from).collect()
    };
    assert_eq!(kept, ["fresh", "live", "recently-used"]);
}
