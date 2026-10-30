use reader_core::journal::*;
use std::fs;
use tempfile::tempdir;
fn note() -> Annotation {
    Annotation {
        id: "note-1".into(),
        body: "请解释失败重试".into(),
        context: "聊天流程".into(),
        path: Some("main.py".into()),
        start_line: Some(2),
        end_line: Some(4),
        file_hash: Some("old-source-hash".into()),
        feature_id: Some("chat".into()),
        feature_name: Some("聊天".into()),
        quote: "submit()\nretry()".into(),
        version: String::new(),
    }
}
fn event(root: &std::path::Path, id: &str) -> Activity {
    Activity {
        project_root: Some(root.canonicalize().unwrap().display().to_string()),
        kind: Some("projectChange".into()),
        files: vec!["src/main.py".into()],
        id: id.into(),
        task: "完善批注".into(),
        stage: "working".into(),
        places: vec!["右侧阅读面板".into()],
        impact: "阅读与反馈".into(),
        expected_effect: "随时记录问题".into(),
        verification: None,
        recorded_at: String::new(),
    }
}
#[test]
fn journal_read_is_readonly_and_agent_never_changes_human_records() {
    let root = tempdir().unwrap();
    assert!(read(root.path()).unwrap().annotations.is_empty());
    assert!(!root.path().join(".reader").exists());
    let saved = save_annotation(root.path(), note(), "").unwrap();
    let before = fs::read(root.path().join(".reader/annotations.json")).unwrap();
    let reported = report(root.path(), event(root.path(), "event-1")).unwrap();
    assert!(!reported.recorded_at.is_empty());
    assert!(reported.verification.is_none());
    assert!(report(root.path(), event(root.path(), "event-1")).is_err());
    assert_eq!(
        before,
        fs::read(root.path().join(".reader/annotations.json")).unwrap()
    );
    assert_eq!(
        read(root.path()).unwrap().annotations[0].version,
        saved.version
    );
    assert!(!root.path().join(".reader/features").exists());
}
#[test]
fn autosave_compare_and_swap_prevents_stale_overwrite_and_delete() {
    let root = tempdir().unwrap();
    let original = save_annotation(root.path(), note(), "").unwrap();
    let mut changed = original.clone();
    changed.body = "新的人工意见".into();
    let current = save_annotation(root.path(), changed.clone(), &original.version).unwrap();
    assert!(save_annotation(root.path(), changed, &original.version).is_err());
    assert!(delete_annotation(root.path(), &original.id, &original.version).is_err());
    assert_eq!(read(root.path()).unwrap().annotations[0].body, current.body);
    delete_annotation(root.path(), &current.id, &current.version).unwrap();
    assert!(read(root.path()).unwrap().annotations.is_empty());
}
#[test]
fn export_preserves_feedback_context_and_historical_source_without_mutating_notes() {
    let root = tempdir().unwrap();
    save_annotation(root.path(), note(), "").unwrap();
    let before = fs::read(root.path().join(".reader/annotations.json")).unwrap();
    let exported = export_file(root.path()).unwrap();
    let md = fs::read_to_string(exported).unwrap();
    for expected in [
        "请解释失败重试",
        "聊天流程",
        "main.py L2–4",
        "old-source-hash",
        "> submit()",
        "> retry()",
        "chat",
    ] {
        assert!(md.contains(expected), "{expected}");
    }
    assert_eq!(
        before,
        fs::read(root.path().join(".reader/annotations.json")).unwrap()
    );
}
#[test]
fn corrupt_or_invalid_input_preserves_existing_journals() {
    let root = tempdir().unwrap();
    report(root.path(), event(root.path(), "valid")).unwrap();
    let mut bad = event(root.path(), "invalid");
    bad.stage = "verified".into();
    assert!(report(root.path(), bad).is_err());
    let mut bad = note();
    bad.path = Some("../outside".into());
    assert!(save_annotation(root.path(), bad, "").is_err());
    fs::write(root.path().join(".reader/annotations.json"), "{broken").unwrap();
    assert!(save_annotation(root.path(), note(), "").is_err());
    assert_eq!(
        fs::read_to_string(root.path().join(".reader/annotations.json")).unwrap(),
        "{broken"
    );
    assert!(read(root.path()).is_err());
}
#[cfg(unix)]
#[test]
fn export_and_save_reject_symlink_targets() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let outside = tempdir().unwrap();
    fs::write(outside.path().join("original"), "do not change").unwrap();
    fs::create_dir(root.path().join(".reader")).unwrap();
    symlink(
        outside.path().join("original"),
        root.path().join(".reader/annotations.md"),
    )
    .unwrap();
    assert!(export_file(root.path()).is_err());
    symlink(
        outside.path().join("original"),
        root.path().join(".reader/annotations.json"),
    )
    .unwrap();
    assert!(save_annotation(root.path(), note(), "").is_err());
    assert_eq!(
        fs::read_to_string(outside.path().join("original")).unwrap(),
        "do not change"
    );
}

#[test]
fn mixed_project_legacy_and_runtime_events_are_not_shown_as_project_changes() {
    let root = tempdir().unwrap();
    let other = tempdir().unwrap();
    let own = event(root.path(), "own");
    let foreign = event(other.path(), "foreign");
    let mut runtime = event(root.path(), "execution");
    runtime.kind = Some("runtimeExecution".into());
    let mut legacy = event(root.path(), "legacy");
    legacy.project_root = None;
    legacy.kind = None;
    legacy.files.clear();
    let mut metadata = event(root.path(), "reader-metadata");
    metadata.files = vec![".reader/agent.json".into()];
    std::fs::create_dir(root.path().join(".reader")).unwrap();
    let path = root.path().join(".reader/activity.json");
    let bytes = serde_json::to_vec(&vec![own, foreign, runtime, legacy, metadata]).unwrap();
    fs::write(&path, &bytes).unwrap();
    fs::write(
        root.path().join(".reader/annotations.json"),
        serde_json::to_vec(&vec![note()]).unwrap(),
    )
    .unwrap();
    fs::write(
        root.path().join(".reader/runs.json"),
        b"protected run bytes",
    )
    .unwrap();
    let journal = read(root.path()).unwrap();
    assert_eq!(
        journal
            .activities
            .iter()
            .map(|a| a.id.as_str())
            .collect::<Vec<_>>(),
        vec!["own"]
    );
    assert_eq!(journal.annotations.len(), 1);
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(
        fs::read(root.path().join(".reader/runs.json")).unwrap(),
        b"protected run bytes"
    );
}

#[test]
fn report_requires_matching_project_scope_and_concrete_source_files() {
    let root = tempdir().unwrap();
    let other = tempdir().unwrap();
    let mut bad = event(other.path(), "wrong-project");
    assert!(report(root.path(), bad.clone()).is_err());
    bad = event(root.path(), "unscoped");
    bad.project_root = None;
    assert!(report(root.path(), bad).is_err());
    bad = event(root.path(), "runtime");
    bad.kind = Some("runtimeExecution".into());
    assert!(report(root.path(), bad).is_err());
    for path in [
        "../other.py",
        "/outside.py",
        ".reader/agent.json",
        "src/.git/config",
        "src/*.py",
    ] {
        bad = event(root.path(), "bad-file");
        bad.files = vec![path.into()];
        assert!(report(root.path(), bad).is_err(), "{path}");
    }
    bad = event(root.path(), "empty-files");
    bad.files.clear();
    assert!(report(root.path(), bad).is_err());
    bad = event(root.path(), "duplicate-files");
    bad.files = vec!["src/logic.py".into(), "src/logic.py".into()];
    assert!(report(root.path(), bad).is_err());
    assert!(!root.path().join(".reader/activity.json").exists());
    let good = report(root.path(), event(root.path(), "planned-file")).unwrap();
    assert_eq!(good.kind.as_deref(), Some("projectChange"));
    assert_eq!(read(root.path()).unwrap().activities.len(), 1);
    assert!(!root.path().join("src/main.py").exists());
}

#[test]
fn legacy_records_remain_readable_without_scope_and_are_not_relabelled() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join(".reader")).unwrap();
    let mut old = serde_json::to_value(event(root.path(), "old")).unwrap();
    for field in ["kind", "projectRoot", "files"] {
        old.as_object_mut().unwrap().remove(field);
    }
    let bytes = serde_json::to_vec(&vec![old]).unwrap();
    let path = root.path().join(".reader/activity.json");
    fs::write(&path, &bytes).unwrap();
    assert!(read(root.path()).unwrap().activities.is_empty());
    assert_eq!(fs::read(path).unwrap(), bytes);
}

#[cfg(unix)]
#[test]
fn activity_files_cannot_cross_a_symlink_to_another_project() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let other = tempdir().unwrap();
    symlink(other.path(), root.path().join("linked")).unwrap();
    let mut bad = event(root.path(), "linked");
    bad.files = vec!["linked/main.py".into()];
    assert!(report(root.path(), bad).is_err());
    assert!(!other.path().join(".reader").exists());
}
