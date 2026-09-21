use super::*;

#[test]
fn test_distribution_quartiles_and_missing_values() {
    let result = distribution([Some(0.0), Some(10.0), None, Some(20.0), Some(30.0)].into_iter());
    assert_eq!(result.n, 4);
    assert_eq!(result.q1, Some(7.5));
    assert_eq!(result.median, Some(15.0));
    assert_eq!(result.q3, Some(22.5));
    assert_eq!(distribution([None].into_iter()).median, None);
}

#[test]
fn test_observation_failed_and_cached_timing() {
    let mut row = json!({"id": Uuid::new_v4(), "status": "completed", "run_duration_s": 100,
        "performance_observation": {"frames": 50, "timing_complete": true},
        "stage_peaks": {"mapping": {"ram_bytes": 0}, "segment": {"ram_bytes": 20}}});
    let result = parse_observation(&row).unwrap();
    assert_eq!(result.seconds_per_frame, Some(2.0));
    assert_eq!(result.ram, Some(20.0));
    row["status"] = json!("failed");
    assert_eq!(
        parse_observation(&row).unwrap().timing_note,
        "Incomplete run"
    );
    assert_eq!(parse_observation(&row).unwrap().seconds_per_frame, None);
    row["status"] = json!("completed");
    row["performance_observation"]["timing_complete"] = json!(false);
    assert_eq!(parse_observation(&row).unwrap().seconds_per_frame, None);
}

fn sample(frames: f64, seconds: f64, ram: f64) -> PerformanceEvidence {
    parse_observation(
        &json!({"id": Uuid::new_v4(), "device_id": "11111111-1111-4111-8111-111111111111",
        "status": "succeeded", "run_duration_s": seconds,
        "performance_observation": {"version": 1, "frames": frames, "timing_complete": true,
            "basis": "process", "hardware": {"total_ram_bytes": 100},
            "settings": {"fps": 5, "processing_width": 1000, "processing_height": 500, "preprocess_batch_size": 4, "mapping_backend": "map", "segmentation_model": "seg", "mode": "semantic"}},
        "stage_peaks": {"mapping": {"ram_bytes": ram, "swap_bytes": 0}}}),
    )
    .unwrap()
}

#[test]
fn test_summary_different_frames_and_per_run_rates() {
    let first = sample(100.0, 100.0, 10.0);
    let second = sample(200.0, 400.0, 30.0);
    assert_eq!(identity(&first), identity(&second));
    let group = summarize(&[&first, &second]);
    assert_eq!(group.stats["ram"].q1, Some(15.0));
    assert_eq!(group.stats["ram"].median, Some(20.0));
    assert_eq!(group.stats["ram"].q3, Some(25.0));
    assert_eq!(group.stats["seconds_per_frame"].median, Some(1.5));
    assert_eq!(group.stats["swap"].median, Some(0.0));
    assert_eq!(group.stats["vram"].n, 0);
    let query = ComparisonQuery {
        min_frames: Some(150),
        ..Default::default()
    };
    assert!(!in_workload(&first, &query));
    assert!(in_workload(&second, &query));
}

#[test]
fn test_comparable_requires_matching_other_settings_and_hardware() {
    let first = sample(100.0, 100.0, 10.0);
    let mut second = sample(200.0, 400.0, 30.0);
    second.settings["fps"] = json!(10);
    assert!(comparable(&first, &second, CompareParameter::Fps));
    assert!(!comparable(&first, &second, CompareParameter::Resolution));
    second.hardware["total_ram_bytes"] = json!(200);
    assert!(!comparable(&first, &second, CompareParameter::Fps));
    second.hardware = first.hardware.clone();
    second.known = false;
    assert!(!comparable(&first, &second, CompareParameter::Fps));
}

#[test]
fn test_identity_keeps_unattributed_devices_and_incomplete_settings_separate() {
    let mut first = sample(100.0, 100.0, 10.0);
    let mut second = sample(200.0, 400.0, 30.0);
    first.device_id = None;
    second.device_id = None;
    assert_ne!(identity(&first), identity(&second));
    assert!(!comparable(&first, &second, CompareParameter::Fps));
    assert!(!settings_known(
        &json!({"version": 1, "settings": {"fps": 5}})
    ));
}
