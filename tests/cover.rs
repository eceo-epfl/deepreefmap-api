//! Pooled cover over a transect: the count-weighted figure, which run each pass
//! contributes, and the per-campaign series.

#[allow(dead_code)]
mod common;

use common::*;

fn uuid(tag: &str) -> String {
    format!("{tag:0>8}-0000-4000-8000-000000000000")
}

async fn create_transect(admin: &axum::Router, name: &str) -> String {
    let (status, body) = post_json(
        admin,
        "/api/transects",
        &serde_json::json!({
            "name": name,
            "description": "",
            "start_lat": 16.2,
            "start_lon": 39.4,
            "end_lat": 16.3,
            "end_lon": 39.5,
        }),
        None,
    )
    .await;
    assert_eq!(status, 201, "transect create failed: {body}");
    body["id"].as_str().expect("id").to_string()
}

fn pass_row(id: &str, transect_id: &str, label: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "transect_id": transect_id,
        "begin_s": 0.0,
        "end_s": 120.0,
        "upside_down": false,
        "label": label,
        "notes": "",
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn run_row(id: &str, pass_id: &str) -> serde_json::Value {
    serde_json::json!({
        "id": id,
        "pass_id": pass_id,
        "status": "succeeded",
        "started_at": "2026-08-01T10:00:00Z",
        "error": "",
        "run_dir_name": id,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn cover_row(
    tag: &str,
    run_id: &str,
    group: &str,
    points: f64,
    denominator: f64,
) -> serde_json::Value {
    serde_json::json!({
        "id": uuid(tag),
        "run_id": run_id,
        "level": "coarse",
        "class_group": group,
        "estimator": "per_pass",
        "fraction": points / denominator,
        "point_count": points,
        "denominator": denominator,
        "created_at": "2026-08-01T00:00:00Z",
        "updated_at": "2026-08-01T10:00:00Z",
    })
}

fn push_body(sections: &serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "contract_version": 1, "sections": sections })
}

async fn create_campaign(admin: &axum::Router, name: &str, begin_date: &str) -> String {
    let (status, body) = post_json(
        admin,
        "/api/campaigns",
        &serde_json::json!({ "name": name, "begin_date": begin_date, "description": "" }),
        None,
    )
    .await;
    assert_eq!(status, 201, "campaign create failed: {body}");
    body["id"].as_str().expect("id").to_string()
}

/// A short pass and a long one over the same line. Pooling by counts must follow the long
/// pass, and averaging the fractions would land halfway between them.
#[tokio::test]
async fn test_pooled_cover_weights_by_counts() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect = create_transect(&admin, "T1").await;
    let short = uuid("a1");
    let long = uuid("a2");
    let short_run = uuid("b1");
    let long_run = uuid("b2");

    let document = push_body(&serde_json::json!({
        "passes": [
            pass_row(&short, &transect, "short"),
            pass_row(&long, &transect, "long"),
        ],
        "runs": [run_row(&short_run, &short), run_row(&long_run, &long)],
        "cover_rows": [
            // 100 points: 90% coral.
            cover_row("e1", &short_run, "coral alive", 90.0, 100.0),
            cover_row("e2", &short_run, "sand", 10.0, 100.0),
            // 900 points: 10% coral.
            cover_row("e3", &long_run, "coral alive", 90.0, 900.0),
            cover_row("e4", &long_run, "sand", 810.0, 900.0),
        ],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (status, pooled) = get_json(
        &admin,
        &format!("/api/transects/{transect}/cover?level=coarse"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{pooled}");

    assert_eq!(pooled["denominator"], 1000.0);
    assert_eq!(pooled["contributing_passes"], 2);
    assert_eq!(pooled["expected_passes"], 2);

    let groups = pooled["groups"].as_array().expect("groups");
    let coral = groups
        .iter()
        .find(|g| g["class_group"] == "coral alive")
        .expect("coral group");
    // 180 of 1000, not the 50% an average of 90% and 10% would give.
    assert_eq!(coral["point_count"], 180.0);
    assert!(
        (coral["fraction"].as_f64().unwrap() - 0.18).abs() < 1e-9,
        "{pooled}"
    );
    // Largest first, so sand leads.
    assert_eq!(groups[0]["class_group"], "sand");
    // Colour comes from the published table, not from rank.
    assert_eq!(coral["colour"], "#e07677");
}

/// A pass processed twice must count once, or the retried pass drags the estimate.
#[tokio::test]
async fn test_pooled_cover_keeps_the_latest_run_per_pass() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect = create_transect(&admin, "T1").await;
    let pass = uuid("c1");
    let first = uuid("d1");
    let second = uuid("d2");

    let mut older = run_row(&first, &pass);
    older["started_at"] = serde_json::json!("2026-08-01T09:00:00Z");
    let mut newer = run_row(&second, &pass);
    newer["started_at"] = serde_json::json!("2026-08-01T15:00:00Z");

    let document = push_body(&serde_json::json!({
        "passes": [pass_row(&pass, &transect, "one")],
        "runs": [older, newer],
        "cover_rows": [
            cover_row("e5", &first, "coral alive", 10.0, 100.0),
            cover_row("e6", &second, "coral alive", 70.0, 100.0),
        ],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (_, pooled) = get_json(&admin, &format!("/api/transects/{transect}/cover"), None).await;
    assert_eq!(pooled["contributing_passes"], 1);
    assert_eq!(pooled["denominator"], 100.0);
    assert_eq!(pooled["groups"][0]["point_count"], 70.0);
}

#[tokio::test]
async fn test_pooled_cover_rejects_an_unknown_level() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let transect = create_transect(&admin, "T1").await;

    let (status, _) = get(
        &admin,
        &format!("/api/transects/{transect}/cover?level=gremlin"),
        None,
    )
    .await;
    assert_eq!(status, 400);
}

/// Passes swum on different expeditions are separate observations of the same line, so a
/// campaign narrows the figure rather than blending into it.
#[tokio::test]
async fn test_pooled_cover_narrows_to_one_campaign() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect = create_transect(&admin, "T1").await;
    let summer = uuid("f1");
    let winter = uuid("f2");
    let summer_pass = uuid("f3");
    let winter_pass = uuid("f4");
    let summer_run = uuid("f5");
    let winter_run = uuid("f6");

    // Campaigns are curated, so they are here before the laptop pushes passes into them.
    for (id, name) in [(&summer, "2023_08_summer"), (&winter, "2024_03_winter")] {
        seed_campaign(&db, id, name).await;
    }

    let mut summer_row = pass_row(&summer_pass, &transect, "summer");
    summer_row["campaign_id"] = serde_json::json!(summer);
    let mut winter_row = pass_row(&winter_pass, &transect, "winter");
    winter_row["campaign_id"] = serde_json::json!(winter);

    let document = push_body(&serde_json::json!({
        "passes": [summer_row, winter_row],
        "runs": [run_row(&summer_run, &summer_pass), run_row(&winter_run, &winter_pass)],
        "cover_rows": [
            cover_row("f7", &summer_run, "coral alive", 80.0, 100.0),
            cover_row("f8", &winter_run, "coral alive", 20.0, 100.0),
        ],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (_, both) = get_json(&admin, &format!("/api/transects/{transect}/cover"), None).await;
    assert_eq!(both["contributing_passes"], 2);
    assert_eq!(both["groups"][0]["point_count"], 100.0);

    let (_, only_summer) = get_json(
        &admin,
        &format!("/api/transects/{transect}/cover?campaign_id={summer}"),
        None,
    )
    .await;
    assert_eq!(only_summer["contributing_passes"], 1);
    assert_eq!(only_summer["expected_passes"], 1);
    assert_eq!(only_summer["groups"][0]["point_count"], 80.0);
    assert_eq!(only_summer["campaign_id"], summer);
}

/// Two campaigns and a pass with none, on one line. The series keeps the three buckets
/// apart and orders them: campaigns by begin date, then the leftover.
#[tokio::test]
async fn test_cover_series_buckets_by_campaign() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let transect = create_transect(&admin, "T1").await;
    let spring = create_campaign(&admin, "2024_04_eilat", "2024-04-02").await;
    let autumn = create_campaign(&admin, "2024_10_eilat", "2024-10-14").await;

    let spring_short = uuid("a1");
    let spring_long = uuid("a2");
    let autumn_pass = uuid("a3");
    let loose_pass = uuid("a5");

    let mut in_spring_short = pass_row(&spring_short, &transect, "spring short");
    in_spring_short["campaign_id"] = serde_json::json!(spring);
    let mut in_spring_long = pass_row(&spring_long, &transect, "spring long");
    in_spring_long["campaign_id"] = serde_json::json!(spring);
    let mut in_autumn = pass_row(&autumn_pass, &transect, "autumn");
    in_autumn["campaign_id"] = serde_json::json!(autumn);

    let document = push_body(&serde_json::json!({
        "passes": [
            in_spring_short,
            in_spring_long,
            in_autumn,
            pass_row(&loose_pass, &transect, "loose"),
        ],
        "runs": [
            run_row(&uuid("b1"), &spring_short),
            run_row(&uuid("b2"), &spring_long),
            run_row(&uuid("b3"), &autumn_pass),
            run_row(&uuid("b5"), &loose_pass),
        ],
        "cover_rows": [
            // 30% then 10% coral: pooled 60 of 400, spread 0.1 to 0.3.
            cover_row("e1", &uuid("b1"), "coral alive", 30.0, 100.0),
            cover_row("e2", &uuid("b2"), "coral alive", 30.0, 300.0),
            cover_row("e3", &uuid("b3"), "coral alive", 50.0, 100.0),
            cover_row("e5", &uuid("b5"), "coral alive", 5.0, 100.0),
        ],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (status, series) = get_json(
        &admin,
        &format!("/api/transects/{transect}/cover-series?level=coarse"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{series}");
    assert_eq!(series["level"], "coarse");
    let entries = series["entries"].as_array().expect("entries");
    assert_eq!(entries.len(), 3, "{series}");

    // The pooled figure follows the long pass, and the spread shows the two runs.
    let first = &entries[0];
    assert_eq!(first["campaign_id"], spring, "{series}");
    assert_eq!(first["campaign_name"], "2024_04_eilat");
    assert_eq!(first["begin_date"], "2024-04-02");
    assert_eq!(first["denominator"], 400.0);
    assert_eq!(first["contributing_passes"], 2);
    let coral = &first["groups"][0];
    assert_eq!(coral["class_group"], "coral alive");
    assert_eq!(coral["point_count"], 60.0);
    assert!(
        (coral["fraction"].as_f64().unwrap() - 0.15).abs() < 1e-9,
        "{series}"
    );
    assert!(
        (coral["min_fraction"].as_f64().unwrap() - 0.1).abs() < 1e-9,
        "{series}"
    );
    assert!(
        (coral["max_fraction"].as_f64().unwrap() - 0.3).abs() < 1e-9,
        "{series}"
    );
    assert_eq!(coral["colour"], "#e07677");

    assert_eq!(entries[1]["campaign_name"], "2024_10_eilat", "{series}");
    assert_eq!(entries[1]["groups"][0]["point_count"], 50.0);

    // No campaign: one shared bucket, last.
    let leftover = &entries[2];
    assert!(leftover["campaign_id"].is_null(), "{series}");
    assert_eq!(leftover["contributing_passes"], 1);
    assert_eq!(leftover["groups"][0]["point_count"], 5.0);
}

#[tokio::test]
async fn test_a_campaign_lists_the_transects_it_surveyed() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let admin = build_test_app_as_admin(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&app, &code).await;

    let first = create_transect(&admin, "T1").await;
    let second = create_transect(&admin, "T2").await;
    let campaign = create_campaign(&admin, "2024_04_eilat", "2024-04-02").await;
    let mut swum = pass_row(&uuid("a1"), &first, "swum");
    swum["campaign_id"] = serde_json::json!(campaign);
    let document = push_body(&serde_json::json!({
        "passes": [swum, pass_row(&uuid("a2"), &second, "elsewhere")],
    }));
    let (status, body) = post_json(&app, "/api/sync/push", &document, Some(&token)).await;
    assert_eq!(status, 200, "{body}");

    let (status, listed) = get_json(
        &admin,
        &format!("/api/campaigns/{campaign}/transects"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{listed}");
    let names: Vec<&str> = listed
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    assert_eq!(names, ["T1"]);
}

#[tokio::test]
async fn test_cover_series_rejects_an_unknown_level() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let transect = create_transect(&admin, "T1").await;

    let (status, _) = get(
        &admin,
        &format!("/api/transects/{transect}/cover-series?level=gremlin"),
        None,
    )
    .await;
    assert_eq!(status, 400);
}
