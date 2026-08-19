//! Device enrolment and the authentication gate.
//!
//! An operator mints a connect code in the web interface, pastes it into the desktop
//! application, and the application trades it for a device token.

#[allow(dead_code)]
mod common;

use common::*;
use deepreefmap_api::common::auth::Role;

#[tokio::test]
async fn test_connect_code_is_single_use() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;

    let (status, first) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({
            "code": code, "device_name": "Field laptop 1",
            "platform": "linux", "gui_version": "0.9.0",
            "library_version": "0.14.2",
        }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{first}");
    assert!(first["token"].as_str().unwrap().starts_with("drmd_"));
    // The device is its own identity, not a delegation of the minter's.
    assert_eq!(first["device_name"], "Field laptop 1");
    assert!(first["user_sub"].is_null(), "{first}");

    let stored: Option<String> = one_value(&db, "SELECT library_version FROM device").await;
    assert_eq!(stored.as_deref(), Some("0.14.2"));

    let (status, second) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({ "code": code, "device_name": "Field laptop 2" }),
        None,
    )
    .await;
    assert_eq!(status, 401, "a spent code must not enrol again: {second}");
}

/// Two laptops pasting one code at the same instant. Sequential single use is not the
/// same property: the read and the argon2 mint sit between the check and the write.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_two_enrolments_racing_one_code_enrol_one_device() {
    let db = setup_test_db().await;
    let code = seed_connect_code(&db, "alice").await;

    let first = build_test_app(db.clone());
    let second = build_test_app(db.clone());
    let body = |name: &str| {
        serde_json::json!({
            "code": code, "device_name": name,
            "platform": "linux", "gui_version": "0.9.0",
        })
    };
    let (one, two) = (body("Laptop 1"), body("Laptop 2"));

    let (left, right) = tokio::join!(
        post_json(&first, "/api/enrol", &one, None),
        post_json(&second, "/api/enrol", &two, None),
    );

    let mut codes = [left.0, right.0];
    codes.sort_unstable();
    assert_eq!(
        codes,
        [200, 401],
        "exactly one enrolment must win: {:?} {:?}",
        left.1,
        right.1
    );

    let devices: i64 = one_value(&db, "SELECT COUNT(*) FROM device").await;
    assert_eq!(devices, 1, "a spent code minted a second device");
}

#[tokio::test]
async fn test_enrol_accepts_wrapper_and_bare_secret() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    // The interface shows the wrapper; operators paste either form.
    let minted = deepreefmap_api::common::tokens::mint_connect_code("http://test.local/api");
    exec(
        &db,
        &format!(
            "INSERT INTO connect_code (id, code_hash, created_by, note, expires_at, created_at) \
             VALUES (gen_random_uuid(), '{}', 'alice', 'test', NOW() + INTERVAL '1 hour', NOW())",
            minted.code_hash
        ),
    )
    .await;

    let (status, body) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({ "code": minted.code, "device_name": "Pasted whole" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn test_enrol_rejects_expired_code() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    let secret = "ab".repeat(32);
    let hash = deepreefmap_api::common::tokens::sha256_hex(&secret);
    exec(
        &db,
        &format!(
            "INSERT INTO connect_code (id, code_hash, created_by, note, expires_at, created_at) \
             VALUES (gen_random_uuid(), '{hash}', 'alice', 'test', \
             NOW() - INTERVAL '1 minute', NOW() - INTERVAL '1 hour')"
        ),
    )
    .await;

    let (status, _) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({ "code": secret, "device_name": "Too late" }),
        None,
    )
    .await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn test_enrol_rejects_malformed_code() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    for code in ["", "not-a-code", "drm1.!!!", &"ff".repeat(32)] {
        let (status, body) = post_json(
            &app,
            "/api/enrol",
            &serde_json::json!({ "code": code, "device_name": "Chancer" }),
            None,
        )
        .await;
        assert_eq!(status, 401, "code {code:?} was not refused: {body}");
        // Unknown, malformed and expired answer alike, so probing tells nothing.
        assert!(
            body["error"].as_str().unwrap().contains("onnect code"),
            "{body}"
        );
    }
}

#[tokio::test]
async fn test_enrol_requires_device_name() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;

    let (status, _) = post_json(
        &app,
        "/api/enrol",
        &serde_json::json!({ "code": code, "device_name": "   " }),
        None,
    )
    .await;
    // A revoke list of blank rows is unusable.
    assert_eq!(status, 400);
}

#[tokio::test]
async fn test_routes_require_authentication() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    for uri in [
        "/api/sites",
        "/api/transects",
        "/api/runs",
        "/api/cover_rows",
        "/api/devices",
        "/api/sync/pull",
        "/api/me",
    ] {
        let (status, _) = get(&app, uri, None).await;
        assert_eq!(status, 401, "{uri} answered without a credential");
    }

    let (status, _) = post(
        &app,
        "/api/sites",
        &serde_json::json!({ "name": "x" }),
        None,
    )
    .await;
    assert_eq!(status, 401);

    // Needed before a client can authenticate at all.
    let (status, _) = get(&app, "/api/config/keycloak", None).await;
    assert_eq!(status, 200);
    let (status, _) = get(&app, "/healthz", None).await;
    assert_eq!(status, 200);
}

#[tokio::test]
async fn test_invalid_bearer_token_refused() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());

    for token in [
        "nonsense",
        "eyJhbGciOiJSUzI1NiJ9.e30.notarealsignature",
        "drmd_short_secret",
        &format!("drmd_{}_{}", "0".repeat(16), "0".repeat(64)),
    ] {
        let (status, _) = get(&app, "/api/me", Some(token)).await;
        assert_eq!(status, 401, "token {token:?} was accepted");
    }
}

#[tokio::test]
async fn test_revoked_device_loses_access() {
    let db = setup_test_db().await;
    let (app, state) = build_test_app_with_state(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Lost laptop").await;

    let (status, _) = get_json(&app, "/api/me", Some(&token)).await;
    assert_eq!(status, 200);

    exec(&db, "UPDATE device SET revoked_at = NOW()").await;
    // The endpoint busts the cache; a direct update does not.
    state.device_token_cache.invalidate_all();

    let (status, _) = get(&app, "/api/me", Some(&token)).await;
    assert_eq!(status, 401);
    let (status, _) = post(
        &app,
        "/api/sync/push",
        &serde_json::json!({ "contract_version": 1, "sections": {} }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 401);
}

#[tokio::test]
async fn test_device_cannot_mint_or_revoke() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    // Otherwise one leaked token mints an endless supply of credentials.
    let (status, body) = post_json(
        &app,
        "/api/devices/connect-codes",
        &serde_json::json!({ "note": "trying my luck" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 403, "{body}");

    let (_, me) = get_json(&app, "/api/me", Some(&token)).await;
    let device_id = me["device_id"].as_str().unwrap();
    let (status, _) = post(
        &app,
        &format!("/api/devices/{device_id}/revoke"),
        &serde_json::json!({}),
        Some(&token),
    )
    .await;
    assert_eq!(status, 403);

    // Attribution must not be editable by the thing being attributed.
    let (status, body) = post(
        &app,
        &format!("/api/devices/{device_id}/rename"),
        &serde_json::json!({ "name": "Someone else's laptop" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 403, "{body}");
    let name: String = one_value(
        &db,
        &format!("SELECT name FROM device WHERE id = '{device_id}'"),
    )
    .await;
    assert_eq!(name, "Field laptop");
}

#[tokio::test]
async fn test_enrolled_by_records_the_minter_without_granting_identity() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    let enrolled_by: String = one_value(&db, "SELECT enrolled_by FROM device").await;
    assert_eq!(enrolled_by, "alice");

    // The audit trail names alice; the credential does not act as her.
    let (status, me) = get_json(&app, "/api/me", Some(&token)).await;
    assert_eq!(status, 200);
    assert!(me["sub"].is_null(), "a device reported a subject: {me}");
    assert_eq!(me["is_device"], true);
    assert_eq!(me["device_name"], "Field laptop");
    assert_eq!(me["is_admin"], false);
}

#[tokio::test]
async fn test_device_refused_on_crud_routes() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    for uri in [
        "/api/sites",
        "/api/campaigns",
        "/api/videos",
        "/api/devices",
    ] {
        let (status, body) = get(&app, uri, Some(&token)).await;
        assert_eq!(status, 403, "{uri} answered a device token: {body}");
    }

    let (status, body) = post(
        &app,
        "/api/sites",
        &serde_json::json!({ "name": "Backdoor", "description": "" }),
        Some(&token),
    )
    .await;
    assert_eq!(status, 403, "{body}");

    // What a device is allowed to reach.
    for uri in ["/api/sync/pull", "/api/me"] {
        let (status, body) = get(&app, uri, Some(&token)).await;
        assert_eq!(status, 200, "{uri} refused a device token: {body}");
    }
}

#[tokio::test]
async fn test_rename_device_allows_the_enroller_and_an_admin() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Typo laptp").await;
    let device_id: String = one_value(&db, "SELECT id::text FROM device").await;

    let alice = build_test_app_as_human(db.clone(), "alice", vec![Role::Member]);
    let (status, body) = post_json(
        &alice,
        &format!("/api/devices/{device_id}/rename"),
        &serde_json::json!({ "name": "Field laptop 1" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["name"], "Field laptop 1");

    // The cache holds the model the token resolved to, name included.
    let (_, me) = get_json(&app, "/api/me", Some(&token)).await;
    assert_eq!(me["device_name"], "Field laptop 1");

    let (status, body) = post_json(
        &build_test_app_as_admin(db.clone()),
        &format!("/api/devices/{device_id}/rename"),
        &serde_json::json!({ "name": "Retired laptop" }),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
}

#[tokio::test]
async fn test_rename_device_rejects_another_member_and_a_blank_name() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    enrol_device(&app, &code, "Alice laptop").await;
    let device_id: String = one_value(&db, "SELECT id::text FROM device").await;

    let bob = build_test_app_as_human(db.clone(), "bob", vec![Role::Member]);
    let (status, body) = post(
        &bob,
        &format!("/api/devices/{device_id}/rename"),
        &serde_json::json!({ "name": "Bob laptop" }),
        None,
    )
    .await;
    assert_eq!(status, 403, "{body}");

    let alice = build_test_app_as_human(db.clone(), "alice", vec![Role::Member]);
    let (status, _) = post(
        &alice,
        &format!("/api/devices/{device_id}/rename"),
        &serde_json::json!({ "name": "   " }),
        None,
    )
    .await;
    assert_eq!(status, 400);

    let name: String = one_value(&db, "SELECT name FROM device").await;
    assert_eq!(name, "Alice laptop");
}

#[tokio::test]
async fn test_device_list_hides_secrets() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice").await;
    let token = enrol_device(&app, &code, "Field laptop").await;

    // Returned once at enrolment; no read path may hand it back.
    let (status, listed) = get_json(&admin, "/api/devices", None).await;
    assert_eq!(status, 200);
    let text = listed.to_string();
    assert!(
        !text.contains("token_hash"),
        "device list exposes token_hash: {text}"
    );
    assert!(
        !text.contains("token_prefix"),
        "device list exposes token_prefix: {text}"
    );
    assert!(
        !text.contains(&token),
        "device list exposes the token itself"
    );
}

/// Scenario: enrolment as deployed, over a real socket with rate limiting on.
///
/// Expected behaviour: a rejected code answers 401. The in-process tests above cannot
/// catch this, because the rate limiter keys on the peer address and only sees one
/// once the router is served with connect info.
#[tokio::test]
async fn test_enrol_answers_over_a_real_socket_with_rate_limiting() {
    let db = setup_test_db().await;

    let mut config = test_config();
    config.disable_rate_limiting = false;
    config.enrol_rate_limit_burst = 5;
    let state = deepreefmap_api::common::AppState::new(db.clone(), config, None);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let server = tokio::spawn(async move {
        axum::serve(listener, deepreefmap_api::routes::build_service(&state))
            .await
            .unwrap();
    });

    let body = format!(r#"{{"code":"{}","device_name":"probe"}}"#, "0".repeat(64));
    let response = reqwest_post(&format!("http://{addr}/api/enrol"), &body).await;

    server.abort();
    assert_eq!(
        response, 401,
        "enrolment answered {response}, so the limiter could not read a peer address"
    );
}

/// Minimal POST over TCP, so the suite gains no HTTP client dependency.
async fn reqwest_post(url: &str, body: &str) -> u16 {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let rest = url.strip_prefix("http://").unwrap();
    let (authority, path) = rest.split_once('/').unwrap();
    let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
    let request = format!(
        "POST /{path} HTTP/1.1\r\nHost: {authority}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();

    let mut raw = String::new();
    stream.read_to_string(&mut raw).await.unwrap();
    raw.split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0)
}
