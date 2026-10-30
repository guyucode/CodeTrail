use reader_core::*;
use std::{fs, path::Path};
use tempfile::TempDir;

const SOURCE: &str = "def normalize_email(value):\n    return value.strip().lower()\n\ndef route_request(message):\n    return normalize_email(message)\n";

fn fixture() -> TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("app.py"), SOURCE).unwrap();
    dir
}

fn draft(name: &str) -> FeatureDraft {
    FeatureDraft {
        id: None,
        name: name.into(),
        description: "理解输入转换与路由".into(),
        color: "#8b5cf6".into(),
        parent_id: None,
    }
}

fn confirm_review(root: &Path, feature_id: &str) -> anyhow::Result<ProjectSnapshot> {
    let snapshot = open_project(root)?;
    let feature = snapshot
        .features
        .iter()
        .find(|f| f.id == feature_id)
        .unwrap();
    set_review(
        root,
        feature_id,
        ReviewState::Verified,
        Some(&feature.current_digest),
    )
}

fn bound(root: &Path) -> String {
    let snapshot = save_feature(root, draft("输入标准化")).unwrap();
    let feature = &snapshot.features[0];
    let symbol = snapshot
        .symbols
        .iter()
        .find(|s| s.name == "normalize_email")
        .unwrap();
    bind_anchor(
        root,
        BindRequest {
            feature_id: feature.id.clone(),
            path: symbol.range.file.clone(),
            start_line: symbol.range.start_line,
            end_line: symbol.range.end_line,
            symbol_id: Some(symbol.id.clone()),
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    feature.id.clone()
}

#[test]
fn parses_scopes_and_real_ranges_for_all_supported_languages() {
    let python = parse_symbols(
        "a.py",
        "class Worker:\n    def run(self):\n        return 1\n\ndef run():\n    return 2\n",
    )
    .unwrap();
    assert_eq!(
        python
            .iter()
            .map(|s| s.qualified_name.as_str())
            .collect::<Vec<_>>(),
        vec!["Worker", "Worker::run", "run"]
    );
    assert_eq!(python[1].range.start_line, 2);
    assert_eq!(python[1].range.end_line, 3);
    let rust = parse_symbols(
        "a.rs",
        "struct Worker;\nimpl Worker {\n    fn run(&self) -> i32 { 1 }\n}\n",
    )
    .unwrap();
    assert!(rust
        .iter()
        .any(|s| s.qualified_name == "Worker::run" && s.range.start_line == 3));
    let ts = parse_symbols(
        "a.ts",
        "export function route(x: string) { return x; }\nconst greet = (name: string) => name;\n",
    )
    .unwrap();
    assert_eq!(
        ts.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(),
        vec!["route", "greet"]
    );
    let js = parse_symbols("a.js", "function route(x) { return x; }\n").unwrap();
    assert_eq!(js[0].name, "route");
}

#[test]
fn opens_are_deterministic_and_create_no_metadata() {
    let dir = fixture();
    let a = open_project(dir.path()).unwrap();
    let b = open_project(dir.path()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a.files.len(), 1);
    assert!(!dir.path().join(".reader").exists());
}

#[test]
fn save_bind_review_and_remove_never_change_source() {
    let dir = fixture();
    let id = bound(dir.path());
    let reviewed = confirm_review(dir.path(), &id).unwrap();
    assert_eq!(reviewed.features[0].review_state, ReviewState::Verified);
    let restored = open_project(dir.path()).unwrap();
    assert_eq!(restored.features, reviewed.features);
    let anchor_id = &restored.features[0].anchors[0].id;
    remove_anchor(dir.path(), &id, anchor_id).unwrap();
    delete_feature(dir.path(), &id).unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("app.py")).unwrap(),
        SOURCE
    );
}

#[test]
fn changed_body_invalidates_review_and_can_be_explicitly_confirmed() {
    let dir = fixture();
    let id = bound(dir.path());
    confirm_review(dir.path(), &id).unwrap();
    fs::write(
        dir.path().join("app.py"),
        SOURCE.replace(".lower()", ".upper()"),
    )
    .unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(snapshot.features[0].review_state, ReviewState::NeedsReview);
    assert_eq!(
        snapshot.features[0].anchors[0].status,
        AnchorStatus::Changed
    );
    let reviewed = confirm_review(dir.path(), &id).unwrap();
    assert_eq!(reviewed.features[0].review_state, ReviewState::Verified);
    assert_eq!(reviewed.features[0].anchors[0].status, AnchorStatus::Exact);
}

#[test]
fn unchanged_symbol_may_move_without_losing_review() {
    let dir = fixture();
    let id = bound(dir.path());
    confirm_review(dir.path(), &id).unwrap();
    fs::write(dir.path().join("app.py"), format!("# heading\n\n{SOURCE}")).unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(snapshot.features[0].anchors[0].range.start_line, 3);
    assert_eq!(snapshot.features[0].anchors[0].status, AnchorStatus::Moved);
    assert_eq!(snapshot.features[0].review_state, ReviewState::Verified);
}

#[test]
fn renamed_symbol_remains_visible_and_cannot_be_marked_verified() {
    let dir = fixture();
    let id = bound(dir.path());
    confirm_review(dir.path(), &id).unwrap();
    fs::write(
        dir.path().join("app.py"),
        SOURCE.replace("normalize_email", "clean_email"),
    )
    .unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(snapshot.features[0].anchors.len(), 1);
    assert_eq!(
        snapshot.features[0].anchors[0].status,
        AnchorStatus::Missing
    );
    assert_eq!(snapshot.features[0].review_state, ReviewState::NeedsReview);
    assert!(confirm_review(dir.path(), &id).is_err());
}

#[test]
fn explicit_rebind_preserves_anchor_id() {
    let dir = fixture();
    let id = bound(dir.path());
    let snapshot = open_project(dir.path()).unwrap();
    let anchor_id = snapshot.features[0].anchors[0].id.clone();
    fs::write(
        dir.path().join("app.py"),
        SOURCE.replace("normalize_email", "clean_email"),
    )
    .unwrap();
    let symbol = open_project(dir.path())
        .unwrap()
        .symbols
        .into_iter()
        .find(|s| s.name == "clean_email")
        .unwrap();
    let rebound = bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: id,
            path: "app.py".into(),
            start_line: 1,
            end_line: 2,
            symbol_id: Some(symbol.id),
            note: None,
            anchor_id: Some(anchor_id.clone()),
        },
    )
    .unwrap();
    assert_eq!(rebound.features[0].anchors[0].id, anchor_id);
    assert_eq!(rebound.features[0].anchors[0].status, AnchorStatus::Exact);
}

#[test]
fn partial_range_relocates_only_when_exact_content_is_unique() {
    let dir = fixture();
    let id = save_feature(dir.path(), draft("一行逻辑"))
        .unwrap()
        .features[0]
        .id
        .clone();
    bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: id.clone(),
            path: "app.py".into(),
            start_line: 2,
            end_line: 2,
            symbol_id: None,
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    fs::write(dir.path().join("app.py"), format!("# new header\n{SOURCE}")).unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(snapshot.features[0].anchors[0].status, AnchorStatus::Moved);
    assert_eq!(snapshot.features[0].anchors[0].range.start_line, 3);
    fs::write(
        dir.path().join("app.py"),
        format!("# new header\n{SOURCE}\ndef other(value):\n    return value.strip().lower()\n"),
    )
    .unwrap();
    assert_eq!(
        open_project(dir.path()).unwrap().features[0].anchors[0].status,
        AnchorStatus::Moved
    );
    fs::write(dir.path().join("app.py"), "# new header\ndef normalize_email(value):\n    return value.strip().lower()\n    return value.strip().lower()\n").unwrap();
    assert_eq!(
        open_project(dir.path()).unwrap().features[0].anchors[0].status,
        AnchorStatus::Ambiguous
    );
}

#[test]
fn paths_cannot_escape_the_opened_root_or_read_internal_metadata() {
    let dir = fixture();
    for path in [
        "../outside",
        "/etc/passwd",
        ".git/config",
        ".reader/index.json",
        "..\\outside",
        "C:/outside",
    ] {
        assert!(read_file(dir.path(), path).is_err(), "accepted {path}");
    }
    assert!(save_feature(
        dir.path(),
        FeatureDraft {
            id: Some("../outside".into()),
            ..draft("invalid")
        }
    )
    .is_err());
}

#[cfg(unix)]
#[test]
fn symlinks_cannot_expand_read_or_write_scope() {
    use std::os::unix::fs::symlink;
    let dir = fixture();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.py"), "SECRET").unwrap();
    symlink(
        outside.path().join("private.py"),
        dir.path().join("linked.py"),
    )
    .unwrap();
    assert!(read_file(dir.path(), "linked.py").is_err());
    assert!(!open_project(dir.path())
        .unwrap()
        .files
        .iter()
        .any(|f| f.path == "linked.py"));
    symlink(outside.path(), dir.path().join(".reader")).unwrap();
    assert!(save_feature(dir.path(), draft("unsafe")).is_err());
    assert!(!outside.path().join("index.json").exists());
}

#[test]
fn malformed_features_are_reported_and_preserved() {
    let dir = fixture();
    bound(dir.path());
    let path = dir.path().join(".reader/features/broken.json");
    fs::write(&path, "not JSON").unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(snapshot.features.len(), 1);
    assert!(snapshot.warnings.iter().any(|w| w.contains("broken.json")));
    assert_eq!(fs::read_to_string(path).unwrap(), "not JSON");
}

#[test]
fn future_schema_is_rejected_without_rewriting() {
    let dir = fixture();
    bound(dir.path());
    let path = dir.path().join(".reader/index.json");
    fs::write(&path, "{\"schemaVersion\":999}").unwrap();
    assert!(open_project(dir.path()).is_err());
    assert!(save_feature(dir.path(), draft("no")).is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "{\"schemaVersion\":999}");
}

#[test]
fn parent_cycles_are_rejected() {
    let dir = fixture();
    let a = save_feature(dir.path(), draft("A")).unwrap().features[0]
        .id
        .clone();
    let b = save_feature(
        dir.path(),
        FeatureDraft {
            parent_id: Some(a.clone()),
            ..draft("B")
        },
    )
    .unwrap()
    .features
    .into_iter()
    .find(|f| f.name == "B")
    .unwrap()
    .id;
    assert!(save_feature(
        dir.path(),
        FeatureDraft {
            id: Some(a),
            parent_id: Some(b),
            ..draft("A")
        }
    )
    .is_err());
}

#[test]
fn generated_directories_and_gitignore_are_excluded() {
    let dir = fixture();
    fs::create_dir(dir.path().join("node_modules")).unwrap();
    fs::write(
        dir.path().join("node_modules/hidden.js"),
        "function ignored() {}",
    )
    .unwrap();
    fs::write(dir.path().join(".gitignore"), "generated.py\n").unwrap();
    fs::write(dir.path().join("generated.py"), "def ignored(): pass").unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(
        snapshot
            .files
            .iter()
            .map(|f| f.path.as_str())
            .collect::<Vec<_>>(),
        vec!["app.py"]
    );
}

#[test]
fn review_export_has_actual_anchors_and_status() {
    let dir = fixture();
    bound(dir.path());
    let report = export_review(dir.path()).unwrap();
    assert!(report.contains("输入标准化"));
    assert!(report.contains("app.py"));
    assert!(report.contains("L1–L2"));
    assert!(!dir.path().join("review.md").exists());
}

#[test]
fn editing_an_unknown_or_corrupt_feature_does_not_overwrite_it() {
    let dir = fixture();
    assert!(save_feature(
        dir.path(),
        FeatureDraft {
            id: Some("unknown".into()),
            ..draft("修改")
        }
    )
    .is_err());
    let id = bound(dir.path());
    let path = dir.path().join(format!(".reader/features/{id}.json"));
    fs::write(&path, "corrupt original").unwrap();
    assert!(save_feature(
        dir.path(),
        FeatureDraft {
            id: Some(id),
            ..draft("修改")
        }
    )
    .is_err());
    assert_eq!(fs::read_to_string(path).unwrap(), "corrupt original");
}

#[test]
fn full_file_selection_accepts_monaco_final_empty_line() {
    let dir = fixture();
    let id = save_feature(dir.path(), draft("完整文件"))
        .unwrap()
        .features[0]
        .id
        .clone();
    let snapshot = bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: id,
            path: "app.py".into(),
            start_line: 1,
            end_line: 6,
            symbol_id: None,
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    assert_eq!(snapshot.features[0].anchors[0].status, AnchorStatus::Exact);
}

#[test]
fn partial_range_never_migrates_into_another_symbol_with_identical_text() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("a.py"),
        "def foo():\n    return 1\n\ndef bar():\n    return 1\n",
    )
    .unwrap();
    let snapshot = save_feature(dir.path(), draft("foo 行为")).unwrap();
    let id = snapshot.features[0].id.clone();
    bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: id.clone(),
            path: "a.py".into(),
            start_line: 2,
            end_line: 2,
            symbol_id: Some("a.py::foo".into()),
            note: None,
            anchor_id: None,
        },
    )
    .unwrap();
    confirm_review(dir.path(), &id).unwrap();
    fs::write(dir.path().join("a.py"), "def bar():\n    return 1\n").unwrap();
    let snapshot = open_project(dir.path()).unwrap();
    assert_eq!(
        snapshot.features[0].anchors[0].status,
        AnchorStatus::Missing
    );
    assert_eq!(snapshot.features[0].review_state, ReviewState::NeedsReview);
    assert!(confirm_review(dir.path(), &id).is_err());
}

#[test]
fn duplicate_bind_uses_resolved_positions_after_code_moved() {
    let dir = fixture();
    let id = bound(dir.path());
    fs::write(dir.path().join("app.py"), format!("# heading\n{SOURCE}")).unwrap();
    assert!(bind_anchor(
        dir.path(),
        BindRequest {
            feature_id: id,
            path: "app.py".into(),
            start_line: 2,
            end_line: 3,
            symbol_id: Some("app.py::normalize_email".into()),
            note: None,
            anchor_id: None
        }
    )
    .is_err());
}

#[test]
fn stale_ui_snapshot_cannot_confirm_changed_code() {
    let dir = fixture();
    let id = bound(dir.path());
    let viewed = confirm_review(dir.path(), &id).unwrap();
    let expected = viewed.features[0].current_digest.clone();
    let metadata_path = dir.path().join(format!(".reader/features/{id}.json"));
    let original_metadata = fs::read(&metadata_path).unwrap();
    fs::write(
        dir.path().join("app.py"),
        SOURCE.replace(".lower()", ".upper()"),
    )
    .unwrap();
    assert!(set_review(dir.path(), &id, ReviewState::Verified, Some(&expected)).is_err());
    assert_eq!(fs::read(&metadata_path).unwrap(), original_metadata);
    let refreshed = open_project(dir.path()).unwrap();
    assert_ne!(refreshed.features[0].current_digest, expected);
    assert_eq!(refreshed.features[0].review_state, ReviewState::NeedsReview);
    let confirmed = confirm_review(dir.path(), &id).unwrap();
    assert_eq!(confirmed.features[0].review_state, ReviewState::Verified);
}

#[test]
fn verified_requires_a_current_digest_but_other_states_do_not() {
    let dir = fixture();
    let id = bound(dir.path());
    assert!(set_review(dir.path(), &id, ReviewState::Verified, None).is_err());
    assert!(set_review(dir.path(), &id, ReviewState::Verified, Some("wrong")).is_err());
    let reading = set_review(dir.path(), &id, ReviewState::Reading, None).unwrap();
    assert_eq!(reading.features[0].review_state, ReviewState::Reading);
    assert_eq!(
        confirm_review(dir.path(), &id).unwrap().features[0].review_state,
        ReviewState::Verified
    );
}

#[test]
fn persisted_current_digest_is_not_trusted_and_legacy_metadata_still_loads() {
    let dir = fixture();
    let id = bound(dir.path());
    let before = open_project(dir.path()).unwrap().features[0]
        .current_digest
        .clone();
    let metadata_path = dir.path().join(format!(".reader/features/{id}.json"));
    let mut data: serde_json::Value =
        serde_json::from_slice(&fs::read(&metadata_path).unwrap()).unwrap();
    data["currentDigest"] = serde_json::json!("forged");
    fs::write(&metadata_path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        open_project(dir.path()).unwrap().features[0].current_digest,
        before
    );
    data.as_object_mut().unwrap().remove("currentDigest");
    fs::write(&metadata_path, serde_json::to_vec(&data).unwrap()).unwrap();
    assert_eq!(
        open_project(dir.path()).unwrap().features[0].current_digest,
        before
    );
}
