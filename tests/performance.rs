//! The fleet performance aggregate: what `/api/performance/summary` folds, and what it
//! ignores.
//!
//! A run's observation is the largest value across its own stages. The published mean,
//! sample standard deviation, min, max and n are then taken across runs over those
//! per-run peaks, so the two levels are asserted separately below.

#[allow(dead_code)]
mod common;

use common::*;

const PASS: &str = "22222222-2222-4222-8222-222222222222";

async fn seed_pass(db: &sea_orm::DatabaseConnection) {
    exec(
        db,
        &format!(
            "INSERT INTO transect_pass (id, begin_s, end_s, created_at, updated_at) \
             VALUES ('{PASS}', 0, 10, NOW(), NOW())"
        ),
    )
    .await;
}

/// Enrol a device under `subject` and stamp it a current profile, returning its id and
/// token.
async fn seed_device(
    db: &sea_orm::DatabaseConnection,
    app: &axum::Router,
    subject: &str,
    name: &str,
    gpu_name: &str,
) -> (String, String) {
    let code = seed_connect_code(db, subject, name).await;
    let token = enrol_device(app, &code).await;
    exec(
        db,
        &format!(
            "UPDATE device SET system_profile = '{{ \
               \"total_ram_bytes\": 34000000000, \
               \"gpu\": {{\"name\": \"{gpu_name}\", \"total_vram_bytes\": 8000000000}} \
             }}'::jsonb WHERE name = '{name}'"
        ),
    )
    .await;
    let id: String = one_value(
        db,
        &format!("SELECT id::text FROM device WHERE name = '{name}'"),
    )
    .await;
    (id, token)
}

#[allow(clippy::too_many_arguments)]
async fn seed_run(
    db: &sea_orm::DatabaseConnection,
    device_id: &str,
    status: &str,
    segmentation: &str,
    mapping: &str,
    started_at: &str,
    duration_s: Option<f64>,
    stage_peaks: &serde_json::Value,
) {
    let duration = duration_s.map_or("NULL".to_string(), |d| d.to_string());
    exec(
        db,
        &format!(
            "INSERT INTO run_record (id, pass_id, status, run_dir_name, device_id, \
             segmentation_model, mapping_backend, preset_name, preset_version, \
             preset_hash, started_at, run_duration_s, stage_peaks, created_at, updated_at) \
             VALUES (gen_random_uuid(), '{PASS}', '{status}', 'run', '{device_id}', \
             '{segmentation}', '{mapping}', 'Standard reef survey', 1, 'abc123', \
             '{started_at}', {duration}, '{stage_peaks}'::jsonb, NOW(), NOW())"
        ),
    )
    .await;
}

/// A run that reports a duration but leaves `stage_peaks` null, as a build that never
/// recorded peaks pushes, and as a run that died before a stage finished does.
async fn seed_run_without_peaks(
    db: &sea_orm::DatabaseConnection,
    device_id: &str,
    status: &str,
    started_at: &str,
    duration_s: f64,
) {
    exec(
        db,
        &format!(
            "INSERT INTO run_record (id, pass_id, status, run_dir_name, device_id, \
             segmentation_model, mapping_backend, preset_name, preset_version, \
             preset_hash, started_at, run_duration_s, created_at, updated_at) \
             VALUES (gen_random_uuid(), '{PASS}', '{status}', 'run', '{device_id}', \
             'segformer-b2', 'scsfmlearner', 'Standard reef survey', 1, 'abc123', \
             '{started_at}', {duration_s}, NOW(), NOW())"
        ),
    )
    .await;
}

/// A run that also records its processing configuration, the GUI's history grain.
///
/// Identical to `seed_run` in every other column, so a group split can only have come
/// from the configuration.
async fn seed_run_with_config(
    db: &sea_orm::DatabaseConnection,
    device_id: &str,
    width: i32,
    height: i32,
    fps: i32,
    batch: i32,
    stage_peaks: &serde_json::Value,
) {
    exec(
        db,
        &format!(
            "INSERT INTO run_record (id, pass_id, status, run_dir_name, device_id, \
             segmentation_model, mapping_backend, preset_name, preset_version, \
             preset_hash, processing_width, processing_height, fps, \
             preprocess_batch_size, started_at, stage_peaks, created_at, updated_at) \
             VALUES (gen_random_uuid(), '{PASS}', 'succeeded', 'run', '{device_id}', \
             'segformer-b2', 'scsfmlearner', 'Standard reef survey', 1, 'abc123', \
             {width}, {height}, {fps}, {batch}, \
             '2026-08-01T00:00:00Z', '{stage_peaks}'::jsonb, NOW(), NOW())"
        ),
    )
    .await;
}

fn peaks(ram: i64, swap: i64, vram: Option<i64>) -> serde_json::Value {
    serde_json::json!({
        "mapping": {
            "ram_bytes": ram,
            "swap_bytes": swap,
            "vram_bytes": vram,
        }
    })
}

/// A sample standard deviation over two observations is irrational, so the hand-computed
/// value is compared within a tolerance rather than exactly.
fn approx(value: &serde_json::Value, expected: f64, what: &str) {
    let got = value
        .as_f64()
        .unwrap_or_else(|| panic!("{what} is not a number: {value}"));
    assert!(
        (got - expected).abs() < 1e-6,
        "{what}: expected {expected}, got {got}"
    );
}

#[tokio::test]
async fn test_summary_groups_by_device_and_model_combo() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;

    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;
    let (beta, _) = seed_device(&db, &device_app, "bob", "Beta", "RTX 3060").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 1, Some(5)),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        Some(300.0),
        &peaks(20, 3, Some(6)),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "loger",
        "2026-08-03T00:00:00Z",
        Some(50.0),
        &peaks(30, 3, Some(7)),
    )
    .await;
    seed_run(
        &db,
        &beta,
        "succeeded",
        "coralscapes-vit-b-dpt",
        "loger",
        "2026-08-04T00:00:00Z",
        Some(70.0),
        &peaks(40, 4, None),
    )
    .await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 3, "{body}");

    // Ordered by device name, then segmentation model, then mapping backend.
    let combo = &groups[0];
    assert_eq!(combo["device_name"], "Alpha");
    assert_eq!(combo["gpu_name"], "RTX 4070");
    assert_eq!(combo["total_ram_bytes"], 34_000_000_000_i64);
    assert_eq!(combo["total_vram_bytes"], 8_000_000_000_i64);
    assert_eq!(combo["segmentation_model"], "segformer-b2");
    assert_eq!(combo["mapping_backend"], "loger");
    assert_eq!(combo["run_count"], 1);

    let combo = &groups[1];
    assert_eq!(combo["device_id"], serde_json::json!(alpha));
    assert_eq!(combo["mapping_backend"], "scsfmlearner");
    assert_eq!(combo["preset_name"], "Standard reef survey");
    assert_eq!(combo["preset_version"], 1);
    assert_eq!(combo["preset_hash"], "abc123");
    assert_eq!(combo["run_count"], 2);
    assert_eq!(combo["failed_count"], 0);
    assert_eq!(combo["ram_max_bytes"], 20);
    assert_eq!(combo["last_run_at"], "2026-08-02T00:00:00Z");

    let combo = &groups[2];
    assert_eq!(combo["device_name"], "Beta");
    assert_eq!(combo["gpu_name"], "RTX 3060");
    assert_eq!(
        combo["vram_n"], 0,
        "a machine with no VRAM figure observes none"
    );
    assert!(combo["vram_mean_bytes"].is_null());
    assert!(combo["vram_max_bytes"].is_null());
}

/// Every metric carries the same five figures over the group's per-run peaks.
///
/// RAM peaks 10 and 20: mean 15, sample std sqrt(((10-15)^2 + (20-15)^2) / 1) = sqrt(50).
/// Swap 1 and 3: mean 2, std sqrt(2). VRAM 5 and 6: mean 5.5, std sqrt(0.5).
/// Durations 100 and 300: mean 200, std sqrt(20000).
#[tokio::test]
async fn test_summary_reports_mean_and_sample_deviation_across_runs() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 1, Some(5)),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        Some(300.0),
        &peaks(20, 3, Some(6)),
    )
    .await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 2);

    approx(&combo["ram_mean_bytes"], 15.0, "ram_mean_bytes");
    approx(
        &combo["ram_std_bytes"],
        7.071_067_811_865_475,
        "ram_std_bytes",
    );
    assert_eq!(combo["ram_min_bytes"], 10);
    assert_eq!(combo["ram_max_bytes"], 20);
    assert_eq!(combo["ram_n"], 2);

    approx(&combo["swap_mean_bytes"], 2.0, "swap_mean_bytes");
    approx(
        &combo["swap_std_bytes"],
        std::f64::consts::SQRT_2,
        "swap_std_bytes",
    );
    assert_eq!(combo["swap_min_bytes"], 1);
    assert_eq!(combo["swap_max_bytes"], 3);
    assert_eq!(combo["swap_n"], 2);

    approx(&combo["vram_mean_bytes"], 5.5, "vram_mean_bytes");
    approx(
        &combo["vram_std_bytes"],
        0.707_106_781_186_547_5,
        "vram_std_bytes",
    );
    assert_eq!(combo["vram_min_bytes"], 5);
    assert_eq!(combo["vram_max_bytes"], 6);
    assert_eq!(combo["vram_n"], 2);

    approx(&combo["duration_mean_s"], 200.0, "duration_mean_s");
    approx(
        &combo["duration_std_s"],
        141.421_356_237_309_5,
        "duration_std_s",
    );
    approx(&combo["duration_min_s"], 100.0, "duration_min_s");
    approx(&combo["duration_max_s"], 300.0, "duration_max_s");
    assert_eq!(combo["duration_n"], 2);
}

/// A run observes the largest stage, not the largest run total, and one run is one
/// observation: mean, min and max collapse onto it and the sample deviation is undefined.
#[tokio::test]
async fn test_summary_takes_the_max_across_stages() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &serde_json::json!({
            "preprocess": { "ram_bytes": 50, "swap_bytes": 9, "vram_bytes": 1 },
            "mapping": { "ram_bytes": 30, "swap_bytes": 2, "vram_bytes": 80 },
        }),
    )
    .await;

    let (_, body) = get_json(&member, "/api/performance/summary", None).await;
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 1);
    approx(&combo["ram_mean_bytes"], 50.0, "ram_mean_bytes");
    assert_eq!(combo["ram_min_bytes"], 50);
    assert_eq!(combo["ram_max_bytes"], 50);
    assert_eq!(combo["ram_n"], 1);
    assert!(
        combo["ram_std_bytes"].is_null(),
        "one observation has no spread"
    );
    assert_eq!(combo["swap_max_bytes"], 9);
    assert!(combo["swap_std_bytes"].is_null());
    assert_eq!(combo["vram_max_bytes"], 80);
    assert!(combo["vram_std_bytes"].is_null());
    approx(&combo["duration_mean_s"], 100.0, "duration_mean_s");
    assert_eq!(combo["duration_n"], 1);
    assert!(combo["duration_std_s"].is_null());
}

/// Inside a stage map, `stage_peaks` arrives from devices, so garbage values are ignored,
/// never a 500. The run still counts, with the metric simply unobserved.
#[tokio::test]
async fn test_summary_ignores_garbage_stage_peaks() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        None,
        &serde_json::json!({
            "preprocess": { "ram_bytes": "lots", "swap_bytes": null, "vram_bytes": [1] },
            "mapping": { "ram_bytes": 30 },
            "ortho": "not even an object",
        }),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        None,
        &serde_json::json!({ "mapping": { "ram_bytes": "lots", "vram_bytes": {} } }),
    )
    .await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 2, "a garbage row still counts as a run");
    approx(&combo["ram_mean_bytes"], 30.0, "ram_mean_bytes");
    assert_eq!(combo["ram_max_bytes"], 30, "the one numeric value survives");
    assert_eq!(combo["ram_n"], 1, "the other run observed no RAM");
    assert_eq!(combo["swap_n"], 0);
    assert!(combo["swap_mean_bytes"].is_null());
    assert_eq!(combo["vram_n"], 0);
    assert!(combo["vram_max_bytes"].is_null());
    assert_eq!(combo["duration_n"], 0);
    assert!(combo["duration_mean_s"].is_null());
}

/// Swap a device could not observe arrives as null, and a build that never measured it
/// omits the key outright. Neither is an observation of zero, so both stay out of `swap_n`
/// and out of the mean, which would otherwise read a third of the one real figure.
#[tokio::test]
async fn test_summary_excludes_null_and_absent_metrics_from_n() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    // RAM is present throughout, so the three runs differ only in how swap arrives.
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        None,
        &serde_json::json!({ "mapping": { "ram_bytes": 10, "swap_bytes": 4 } }),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        None,
        &serde_json::json!({ "mapping": { "ram_bytes": 20, "swap_bytes": null } }),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-03T00:00:00Z",
        None,
        &serde_json::json!({ "mapping": { "ram_bytes": 30 } }),
    )
    .await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 3);
    assert_eq!(combo["ram_n"], 3);
    approx(&combo["ram_mean_bytes"], 20.0, "ram_mean_bytes");

    assert_eq!(
        combo["swap_n"], 1,
        "null swap and absent swap are both unobserved"
    );
    approx(&combo["swap_mean_bytes"], 4.0, "swap_mean_bytes");
    assert_eq!(combo["swap_min_bytes"], 4, "no zero dragged the floor down");
    assert_eq!(combo["swap_max_bytes"], 4);
    assert!(combo["swap_std_bytes"].is_null());
}

/// An out-of-memory run's peaks are exactly the interesting ones.
///
/// The failed run below carries peaks because the desktop application records them on the
/// failure path, not only once a run returns. This is a shape the fleet produces, not a
/// fixture standing in for one the producer cannot reach.
#[tokio::test]
async fn test_summary_includes_failed_runs() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 0, Some(5)),
    )
    .await;
    seed_run(
        &db,
        &alpha,
        "failed",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        None,
        &peaks(90, 0, Some(99)),
    )
    .await;

    let (_, body) = get_json(&member, "/api/performance/summary", None).await;
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 2);
    assert_eq!(combo["failed_count"], 1);
    approx(&combo["ram_mean_bytes"], 50.0, "ram_mean_bytes");
    assert_eq!(combo["ram_max_bytes"], 90, "the failed run's peak counts");
    assert_eq!(combo["ram_n"], 2);
    assert_eq!(combo["vram_max_bytes"], 99);
    assert_eq!(
        combo["duration_n"], 1,
        "the failed run never reported a duration"
    );
}

/// A run that recorded no peaks at all is absent, not a member with nothing to report.
///
/// Expected behaviour: it stays out of `run_count`, out of `failed_count` and out of
/// every metric's n, including the duration it did report, and it forms no group of its
/// own. The figures then describe only the runs they were computed from.
///
/// The shapes below all say the same thing as an absent column, so the aggregate treats
/// them the same way. A device can push `{}` or a scalar through the JSON column, and
/// counting either would make `run_count` turn on how the laptop serialised "nothing".
#[tokio::test]
async fn test_summary_excludes_runs_without_stage_peaks() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 0, Some(5)),
    )
    .await;
    seed_run_without_peaks(&db, &alpha, "succeeded", "2026-08-02T00:00:00Z", 900.0).await;
    seed_run_without_peaks(&db, &alpha, "failed", "2026-08-03T00:00:00Z", 5.0).await;

    let unreadable = [
        serde_json::json!(null),
        serde_json::json!({}),
        serde_json::json!("scalar peaks"),
        serde_json::json!([{ "ram_bytes": 9000 }]),
    ];
    for (offset, stage_peaks) in unreadable.iter().enumerate() {
        seed_run(
            &db,
            &alpha,
            "failed",
            "segformer-b2",
            "scsfmlearner",
            &format!("2026-08-1{offset}T00:00:00Z"),
            Some(7.0),
            stage_peaks,
        )
        .await;
    }

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1, "the peakless runs formed no group: {body}");

    let combo = &groups[0];
    assert_eq!(combo["run_count"], 1);
    assert_eq!(
        combo["failed_count"], 0,
        "a failed run with no peaks is not a failure this aggregate saw"
    );
    assert_eq!(combo["ram_n"], 1);
    approx(&combo["ram_mean_bytes"], 10.0, "ram_mean_bytes");
    assert_eq!(combo["duration_n"], 1);
    approx(&combo["duration_max_s"], 100.0, "duration_max_s");
    assert_eq!(
        combo["last_run_at"], "2026-08-01T00:00:00Z",
        "a later peakless run does not move the group's last run"
    );
}

/// A machine with no discrete GPU reports no VRAM, so n is per metric, not per group.
#[tokio::test]
async fn test_summary_counts_vram_only_where_a_card_reported_it() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(10, 0, Some(4))).await;
    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(20, 0, Some(8))).await;
    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(30, 0, None)).await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let combo = &body["groups"][0];
    assert_eq!(combo["run_count"], 3);
    assert_eq!(combo["ram_n"], 3);
    approx(&combo["ram_mean_bytes"], 20.0, "ram_mean_bytes");
    assert_eq!(
        combo["vram_n"], 2,
        "the card-less run is not an observation"
    );
    approx(&combo["vram_mean_bytes"], 6.0, "vram_mean_bytes");
    approx(
        &combo["vram_std_bytes"],
        2.828_427_124_746_19,
        "vram_std_bytes",
    );
    assert_eq!(combo["vram_min_bytes"], 4);
    assert_eq!(combo["vram_max_bytes"], 8);
}

/// Resolution changes the memory regime, so it splits the group, as fps and batch do.
#[tokio::test]
async fn test_summary_splits_on_processing_config() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(10, 0, Some(5))).await;
    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(20, 0, Some(6))).await;
    seed_run_with_config(&db, &alpha, 1280, 704, 4, 8, &peaks(90, 0, Some(50))).await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 2, "{body}");

    let small = &groups[0];
    assert_eq!(small["processing_width"], 640);
    assert_eq!(small["processing_height"], 352);
    assert_eq!(small["fps"], 4);
    assert_eq!(small["preprocess_batch_size"], 8);
    assert_eq!(small["run_count"], 2);
    approx(&small["ram_mean_bytes"], 15.0, "ram_mean_bytes");
    assert_eq!(small["ram_max_bytes"], 20);

    let large = &groups[1];
    assert_eq!(large["processing_width"], 1280);
    assert_eq!(large["run_count"], 1);
    approx(&large["ram_mean_bytes"], 90.0, "ram_mean_bytes");
    assert_eq!(large["ram_max_bytes"], 90);
    assert!(large["ram_std_bytes"].is_null());
}

/// Runs recorded before these columns existed group together, not with any config.
#[tokio::test]
async fn test_summary_pools_legacy_runs_in_a_null_config_group() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run_with_config(&db, &alpha, 640, 352, 4, 8, &peaks(10, 0, Some(5))).await;
    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-02T00:00:00Z",
        None,
        &peaks(30, 0, Some(7)),
    )
    .await;

    let (_, body) = get_json(&member, "/api/performance/summary", None).await;
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 2, "{body}");

    // Config groups order before the null-config legacy group.
    assert_eq!(groups[0]["processing_width"], 640);
    assert_eq!(groups[0]["ram_max_bytes"], 10);
    let legacy = &groups[1];
    assert!(legacy["processing_width"].is_null());
    assert!(legacy["processing_height"].is_null());
    assert!(legacy["fps"].is_null());
    assert!(legacy["preprocess_batch_size"].is_null());
    assert_eq!(legacy["run_count"], 1);
    approx(&legacy["ram_mean_bytes"], 30.0, "ram_mean_bytes");
    assert_eq!(legacy["ram_max_bytes"], 30);
}

/// Retiring a laptop must not rewrite what its hardware cost, so nothing here filters on
/// `revoked_at`. The join to `device` is the only place a revocation could bite.
#[tokio::test]
async fn test_summary_keeps_a_revoked_devices_history() {
    let db = setup_test_db().await;
    let admin = build_test_app_as_admin(db.clone());
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Retired laptop", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 0, Some(5)),
    )
    .await;

    let (status, body) = post_json(
        &admin,
        &format!("/api/devices/{alpha}/revoke"),
        &serde_json::json!({}),
        None,
    )
    .await;
    assert_eq!(status, 200, "{body}");
    let revoked: Option<String> = one_value(&db, "SELECT revoked_at::text FROM device").await;
    assert!(revoked.is_some(), "the laptop was not retired");

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    let groups = body["groups"].as_array().expect("groups");
    assert_eq!(groups.len(), 1, "the retired laptop's history went: {body}");

    let combo = &groups[0];
    assert_eq!(combo["device_id"], serde_json::json!(alpha));
    assert_eq!(combo["device_name"], "Retired laptop");
    assert_eq!(
        combo["gpu_name"], "RTX 4070",
        "the device row still resolves after revocation"
    );
    assert_eq!(combo["run_count"], 1);
    assert_eq!(combo["ram_max_bytes"], 10);
}

#[tokio::test]
async fn test_summary_excludes_tombstoned_runs() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (alpha, _) = seed_device(&db, &device_app, "alice", "Alpha", "RTX 4070").await;

    seed_run(
        &db,
        &alpha,
        "succeeded",
        "segformer-b2",
        "scsfmlearner",
        "2026-08-01T00:00:00Z",
        Some(100.0),
        &peaks(10, 0, Some(5)),
    )
    .await;
    exec(&db, "UPDATE run_record SET deleted_at = NOW()").await;

    let (status, body) = get_json(&member, "/api/performance/summary", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["groups"], serde_json::json!([]));
}

#[tokio::test]
async fn test_summary_refuses_a_device_token() {
    let db = setup_test_db().await;
    let device_app = build_test_app(db.clone());
    let code = seed_connect_code(&db, "alice", "Field laptop").await;
    let token = enrol_device(&device_app, &code).await;

    let (status, body) = get(&device_app, "/api/performance/summary", Some(&token)).await;
    assert_eq!(status, 403, "a device browsed the fleet: {body}");
}

#[tokio::test]
async fn test_comparison_consolidates_workloads_and_filters_evidence() {
    let db = setup_test_db().await;
    let member = build_test_app_as_member(db.clone());
    let device_app = build_test_app(db.clone());
    seed_pass(&db).await;
    let (device, _) = seed_device(&db, &device_app, "comparison", "Comparison", "GPU").await;
    for (duration, ram) in [(100.0, 10), (400.0, 30)] {
        seed_run(
            &db,
            &device,
            "succeeded",
            "seg",
            "map",
            "2026-09-21T12:00:00Z",
            Some(duration),
            &peaks(ram, 0, None),
        )
        .await;
    }
    exec(&db, "UPDATE run_record SET performance_observation = jsonb_build_object(
        'version', 1, 'settings', jsonb_build_object('fps', 5, 'processing_width', 1000, 'processing_height', 500,
            'preprocess_batch_size', 4, 'mapping_backend', 'map', 'segmentation_model', 'seg', 'mode', 'semantic'),
        'hardware', jsonb_build_object('total_ram_bytes', 100), 'basis', 'process',
        'timing_complete', true, 'frames', CASE WHEN run_duration_s = 100 THEN 100 ELSE 200 END)").await;
    let (status, body) = get_json(&member, "/api/performance/comparison", None).await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["configurations"].as_array().unwrap().len(), 1);
    assert_eq!(body["baseline"]["count"], 2);
    assert_eq!(body["groups"][0]["count"], 2);
    assert_eq!(body["baseline"]["stats"]["ram"]["median"], 20.0);
    assert_eq!(
        body["baseline"]["stats"]["seconds_per_frame"]["median"],
        1.5
    );
    let id = body["baseline"]["configuration"]["id"].as_str().unwrap();
    let (status, evidence) = get_json(
        &member,
        &format!("/api/performance/evidence?baseline={id}&min_frames=150"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{evidence}");
    assert_eq!(evidence["total"], 1);
    assert_eq!(evidence["rows"][0]["frames"], 200.0);
    let (_, empty) = get_json(
        &member,
        &format!("/api/performance/evidence?baseline={id}&offset=50"),
        None,
    )
    .await;
    assert_eq!(empty["total"], 2);
    assert_eq!(empty["rows"].as_array().unwrap().len(), 0);
    let (status, _) = get_json(
        &member,
        "/api/performance/comparison?min_frames=20&max_frames=10",
        None,
    )
    .await;
    assert_eq!(status, 400);
    seed_run(
        &db,
        &device,
        "succeeded",
        "seg",
        "map",
        "2026-09-22T12:00:00Z",
        Some(50.0),
        &peaks(5, 0, None),
    )
    .await;
    exec(&db, "UPDATE run_record SET performance_observation = jsonb_set(
        (SELECT performance_observation FROM run_record WHERE run_duration_s = 100), '{settings,fps}', '10'::jsonb)
        WHERE run_duration_s = 50").await;
    let (status, compared) = get_json(
        &member,
        &format!("/api/performance/comparison?baseline={id}&parameter=fps"),
        None,
    )
    .await;
    assert_eq!(status, 200, "{compared}");
    assert_eq!(compared["alternatives"].as_array().unwrap().len(), 1);
    let (_, different) = get_json(
        &member,
        &format!("/api/performance/comparison?baseline={id}&parameter=resolution"),
        None,
    )
    .await;
    assert!(different["alternatives"].as_array().unwrap().is_empty());
    assert_eq!(different["groups"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn test_performance_observation_sync_preserves_older_clients() {
    let db = setup_test_db().await;
    let app = build_test_app(db.clone());
    let member = build_test_app_as_member(db.clone());
    seed_pass(&db).await;
    let (_, token) = seed_device(&db, &app, "sync-performance", "Sync performance", "GPU").await;
    let id = uuid::Uuid::new_v4().to_string();
    let meta = serde_json::json!({"version": 1, "basis": "process", "frames": 150,
        "timing_complete": true, "hardware": {"total_ram_bytes": 100},
        "settings": {"processing_width": 1000, "processing_height": 500, "fps": 5,
            "preprocess_batch_size": 4, "mapping_backend": "map", "segmentation_model": "seg", "mode": "semantic"}});
    let row = serde_json::json!({"id": id, "pass_id": PASS, "status": "succeeded",
        "run_dir_name": "run", "error": "", "created_at": "2026-09-21T12:00:00Z",
        "updated_at": "2026-09-21T12:00:00Z", "run_duration_s": 300,
        "stage_peaks": {"mapping": {"ram_bytes": 20}}, "performance_observation": meta});
    let mut headers = negotiation();
    headers[0].1 = "1-2".to_owned();
    let body = serde_json::json!({"contract_version": 2, "sections": {"runs": [row]}});
    let (status, _, pushed) =
        post_declaring(&app, "/api/sync/push", &body, Some(&token), &headers).await;
    assert_eq!(status, 200, "{pushed}");
    let (status, summary) = get_json(&member, "/api/performance/comparison", None).await;
    assert_eq!(status, 200, "{summary}");
    assert_eq!(
        summary["baseline"]["configuration"]["known"], true,
        "{summary}"
    );
    assert_eq!(
        summary["baseline"]["stats"]["seconds_per_frame"]["median"],
        2.0
    );
    let (status, _, pulled) =
        get_declaring(&app, "/api/sync/pull?since=0", Some(&token), &headers).await;
    assert_eq!(status, 200, "{pulled}");
    let document: serde_json::Value = serde_json::from_str(&pulled).unwrap();
    assert_eq!(
        document["sections"]["runs"][0]["performance_observation"],
        meta
    );
    let (_, older) = get_json(&app, "/api/sync/pull?since=0", Some(&token)).await;
    assert!(
        older["sections"]["runs"][0]
            .get("performance_observation")
            .is_none()
    );
}
