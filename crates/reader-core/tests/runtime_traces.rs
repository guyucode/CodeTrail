use reader_core::trace::*;
use std::fs;
fn setup() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("logic.py"), "def run():\n    return 7\n").unwrap();
    dir
}
fn start_request(id: &str) -> RunStart {
    RunStart {
        id: id.into(),
        label: "实际隔离运行".into(),
        kind: "isolatedTest".into(),
        producer: "test".into(),
        summary: "真实执行测试代码，不调用模型。".into(),
    }
}
fn step(root: &std::path::Path, run_id: &str, id: &str) -> StepRequest {
    StepRequest {
        run_id: run_id.into(),
        step: RuntimeStep {
            id: id.into(),
            title: "得到返回值".into(),
            status: "ok".into(),
            result: "返回 7".into(),
            reason: "实际调用完成。".into(),
            inputs: vec![],
            outputs: vec![ObservedValue {
                name: "result".into(),
                value: "7".into(),
            }],
            elapsed_ms: 2,
            location: RuntimeLocation {
                path: "logic.py".into(),
                start_line: 1,
                end_line: 2,
                file_hash: reader_core::read_file(root, "logic.py").unwrap().hash,
            },
        },
    }
}
#[test]
fn reads_are_readonly_and_append_keeps_frozen_code_when_source_changes() {
    let dir = setup();
    let root = dir.path();
    assert!(list(root).unwrap().is_empty());
    assert!(!root.join(".reader").exists());
    start(root, start_request("one")).unwrap();
    append(root, step(root, "one", "first")).unwrap();
    finish(
        root,
        RunFinish {
            verification: None,
            run_id: "one".into(),
            status: "completed".into(),
            summary: "实际结束".into(),
        },
    )
    .unwrap();
    fs::write(root.join("logic.py"), "def run():\n    return 99\n").unwrap();
    let view = read(root, "one").unwrap();
    assert!(!view.steps[0].source_matches);
    assert!(view.steps[0].record.source_excerpt.contains("return 7"));
    assert_eq!(view.steps[0].record.observation.outputs[0].value, "7");
    assert_eq!(view.run.status, "completed");
}
#[test]
fn lifecycle_duplicate_and_separate_runs_are_enforced() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("one")).unwrap();
    start(root, start_request("two")).unwrap();
    assert!(start(root, start_request("one")).is_err());
    let first = append(root, step(root, "one", "first")).unwrap();
    assert_eq!(first.sequence, 1);
    assert!(append(root, step(root, "one", "first")).is_err());
    assert_eq!(
        append(root, step(root, "two", "first")).unwrap().sequence,
        1
    );
    finish(
        root,
        RunFinish {
            verification: None,
            run_id: "one".into(),
            status: "failed".into(),
            summary: "执行失败".into(),
        },
    )
    .unwrap();
    assert!(append(root, step(root, "one", "late")).is_err());
    assert!(finish(
        root,
        RunFinish {
            verification: None,
            run_id: "one".into(),
            status: "completed".into(),
            summary: "不能覆写".into()
        }
    )
    .is_err());
    assert_eq!(read(root, "two").unwrap().run.status, "running");
}
#[test]
fn stale_or_outside_locations_preserve_store_and_human_records() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("one")).unwrap();
    fs::write(root.join(".reader/annotations.json"), "[]").unwrap();
    let before = fs::read(root.join(".reader/runs.json")).unwrap();
    let mut bad = step(root, "one", "wrong");
    bad.step.location.file_hash = "old".into();
    assert!(append(root, bad).is_err());
    let mut bad = step(root, "one", "wrong");
    bad.step.location.path = "../outside".into();
    assert!(append(root, bad).is_err());
    let mut bad = step(root, "one", "wrong");
    bad.step.location.end_line = 999;
    assert!(append(root, bad).is_err());
    assert_eq!(before, fs::read(root.join(".reader/runs.json")).unwrap());
    assert_eq!(
        fs::read_to_string(root.join(".reader/annotations.json")).unwrap(),
        "[]"
    );
    assert!(!root.join(".reader/features").exists());
    assert!(!root.join(".reader/agent.json").exists());
}
#[test]
fn corrupt_store_and_size_limits_are_visible_without_overwrite() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("one")).unwrap();
    let mut bad = step(root, "one", "huge");
    bad.step.outputs[0].value = "x".repeat(2001);
    assert!(append(root, bad).is_err());
    fs::write(root.join(".reader/runs.json"), "{broken").unwrap();
    assert!(start(root, start_request("two")).is_err());
    assert!(list(root).is_err());
    fs::write(
        root.join(".reader/runs.json"),
        r#"{"schemaVersion":99,"runs":[]}"#,
    )
    .unwrap();
    assert!(list(root).is_err());
    fs::write(root.join(".reader/runs.json"), "{broken").unwrap();
    assert_eq!(
        fs::read_to_string(root.join(".reader/runs.json")).unwrap(),
        "{broken"
    );
}
#[cfg(unix)]
#[test]
fn symlink_store_and_source_are_rejected() {
    use std::os::unix::fs::symlink;
    let dir = setup();
    let outside = setup();
    let root = dir.path();
    start(root, start_request("one")).unwrap();
    symlink(outside.path().join("logic.py"), root.join("linked.py")).unwrap();
    let mut bad = step(root, "one", "link");
    bad.step.location.path = "linked.py".into();
    assert!(append(root, bad).is_err());
    fs::remove_file(root.join(".reader/runs.json")).unwrap();
    symlink(
        outside.path().join("logic.py"),
        root.join(".reader/runs.json"),
    )
    .unwrap();
    assert!(start(root, start_request("two")).is_err());
    assert_eq!(
        fs::read_to_string(outside.path().join("logic.py")).unwrap(),
        "def run():\n    return 7\n"
    );
}
#[test]
fn parallel_writers_preserve_all_steps_and_unique_sequences() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("one")).unwrap();
    std::thread::scope(|scope| {
        for n in 0..8 {
            let path = root.to_path_buf();
            scope.spawn(move || {
                append(&path, step(&path, "one", &format!("step-{n}"))).unwrap();
            });
        }
    });
    let view = read(root, "one").unwrap();
    assert_eq!(view.steps.len(), 8);
    assert_eq!(
        view.steps
            .iter()
            .map(|s| s.record.sequence)
            .collect::<Vec<_>>(),
        (1..=8).collect::<Vec<_>>()
    );
}

#[test]
fn verification_summary_preserves_legacy_and_retains_actual_values_after_changes() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("legacy")).unwrap();
    append(root, step(root, "legacy", "old")).unwrap();
    finish(
        root,
        RunFinish {
            run_id: "legacy".into(),
            status: "completed".into(),
            summary: "Old run".into(),
            verification: None,
        },
    )
    .unwrap();
    assert!(!fs::read_to_string(root.join(".reader/runs.json"))
        .unwrap()
        .contains("verification"));
    start(root, start_request("new")).unwrap();
    append(root, step(root, "new", "actual")).unwrap();
    let v = Verification {
        goal: "Compare actual result".into(),
        inputs: vec![],
        expected: "8".into(),
        actual: "7".into(),
        key_values: vec![ObservedValue {
            name: "returned".into(),
            value: "7".into(),
        }],
        verdict: "different".into(),
    };
    finish(
        root,
        RunFinish {
            run_id: "new".into(),
            status: "completed".into(),
            summary: "Real function returned 7".into(),
            verification: Some(v),
        },
    )
    .unwrap();
    assert!(read(root, "legacy").unwrap().run.verification.is_none());
    fs::write(root.join("logic.py"), "def run():\n    return 99\n").unwrap();
    let view = read(root, "new").unwrap();
    assert!(!view.steps[0].source_matches);
    assert_eq!(view.run.verification.unwrap().actual, "7");
    assert!(view.steps[0].record.source_excerpt.contains("return 7"));
}
#[test]
fn malformed_verification_does_not_overwrite_a_running_record() {
    let dir = setup();
    let root = dir.path();
    start(root, start_request("run")).unwrap();
    append(root, step(root, "run", "one")).unwrap();
    let before = fs::read(root.join(".reader/runs.json")).unwrap();
    let mut request = RunFinish {
        run_id: "run".into(),
        status: "completed".into(),
        summary: "summary".into(),
        verification: Some(Verification {
            goal: "goal".into(),
            inputs: vec![],
            expected: "expected".into(),
            actual: "actual".into(),
            key_values: vec![],
            verdict: "guessed".into(),
        }),
    };
    assert!(finish(root, request.clone()).is_err());
    assert_eq!(before, fs::read(root.join(".reader/runs.json")).unwrap());
    request.verification.as_mut().unwrap().verdict = "matched".into();
    request.status = "failed".into();
    assert!(finish(root, request.clone()).is_err());
    request.status = "completed".into();
    request.verification.as_mut().unwrap().key_values = (0..6)
        .map(|i| ObservedValue {
            name: i.to_string(),
            value: "x".into(),
        })
        .collect();
    assert!(finish(root, request).is_err());
    assert_eq!(before, fs::read(root.join(".reader/runs.json")).unwrap());
    assert!(capture_location(root, ".reader/runs.json", 1, 1).is_err());
    assert!(capture_location(root, "logic.py", 1, 201).is_err());
}
