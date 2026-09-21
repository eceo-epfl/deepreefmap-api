//! The processing history of one clip: `/api/videos/{id}/runs`.

#[allow(dead_code)]
mod common;

use common::*;

fn uuid(tag: &str) -> String {
    format!("{tag:0>8}-0000-4000-8000-000000000000")
}

fn video_row(id: &str, file_name: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "file_name": file_name,
        "gravity": "unknown",
        "gps": "unknown",
        "upside_down": false,
        "review": "unreviewed",
        "notes": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn pass_row(id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "begin_s": 0.0,
        "end_s": 120.0,
        "upside_down": false,
        "label": "",
        "notes": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn pass_video_row(id: &str, pass_id: &str, video_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "pass_id": pass_id,
        "video_id": video_id,
        "ordinal": 0,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn run_row(id: &str, pass_id: &str, started_at: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "pass_id": pass_id,
        "status": "succeeded",
        "started_at": started_at,
        "error": "",
        "run_dir_name": id,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

/// Reruns of one pass and a run of another clip: only the clip's own history comes
/// back, newest first with an unstarted run last.
#[tokio::test]
async fn test_video_runs_lists_the_consuming_runs_newest_first() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let video = uuid("a1");
    let other_video = uuid("a2");
    let pass = uuid("b1");
    let other_pass = uuid("b2");
    let early = uuid("c1");
    let late = uuid("c2");
    let unstarted = uuid("c3");
    let unrelated = uuid("c4");

    let document = push_body(&serde_json::json!({
        "videos": [video_row(&video, "GX010001.MP4"), video_row(&other_video, "GX010002.MP4")],
        "passes": [pass_row(&pass), pass_row(&other_pass)],
        "pass_videos": [
            pass_video_row(&uuid("d1"), &pass, &video),
            pass_video_row(&uuid("d2"), &other_pass, &other_video),
        ],
        "runs": [
            run_row(&early, &pass, Some("2026-08-01T09:00:00Z")),
            run_row(&late, &pass, Some("2026-08-01T15:00:00Z")),
            run_row(&unstarted, &pass, None),
            run_row(&unrelated, &other_pass, Some("2026-08-01T12:00:00Z")),
        ],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(&admin, &format!("/api/videos/{video}/runs"), None).await;
    assert_eq!(status, 200, "{listed}");
    let ids: Vec<&str> = listed
        .as_array()
        .expect("a list of runs")
        .iter()
        .map(|r| r["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids, vec![late.as_str(), early.as_str(), unstarted.as_str()]);
}

#[tokio::test]
async fn test_video_runs_hides_tombstoned_runs() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let video = uuid("a1");
    let pass = uuid("b1");
    let run = uuid("c1");

    let document = push_body(&serde_json::json!({
        "videos": [video_row(&video, "GX010001.MP4")],
        "passes": [pass_row(&pass)],
        "pass_videos": [pass_video_row(&uuid("d1"), &pass, &video)],
        "runs": [run_row(&run, &pass, Some("2026-08-01T09:00:00Z"))],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let mut tombstoned = run_row(&run, &pass, Some("2026-08-01T09:00:00Z"));
    tombstoned["updated_at"] = serde_json::json!("2026-08-01T11:00:00Z");
    tombstoned["deleted_at"] = serde_json::json!("2026-08-01T11:00:00Z");
    let (status, body) = post_json(
        &app,
        "/api/sync/push",
        &push_body(&serde_json::json!({ "runs": [tombstoned] })),
        Some(&token),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(&admin, &format!("/api/videos/{video}/runs"), None).await;
    assert_eq!(status, 200, "{listed}");
    assert!(
        listed.as_array().unwrap().is_empty(),
        "a tombstoned run resurfaced: {listed}"
    );
}

#[tokio::test]
async fn test_video_runs_refuses_a_device_and_answers_empty_for_an_unknown_video() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let video = uuid("a1");
    let (status, body) = get(&app, &format!("/api/videos/{video}/runs"), Some(&token)).await;
    assert_eq!(status, 403, "a device read a console view: {body}");

    let (status, listed) = get_json(&admin, &format!("/api/videos/{video}/runs"), None).await;
    assert_eq!(status, 200);
    assert!(listed.as_array().unwrap().is_empty(), "{listed}");
}
