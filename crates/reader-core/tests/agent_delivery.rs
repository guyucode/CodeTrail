use reader_core::{agent, *};
use serde_json::{json, Value};
use std::{fs, path::Path};

fn fixture() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("logic.py"), "def run():\n    return 1\n").unwrap();
    dir
}

#[test]
fn binary_assets_are_informational_but_incomplete_index_warnings_block() {
    let dir = fixture();
    fs::write(dir.path().join("icon.png"), [0, 1, 2, 0]).unwrap();
    let project = send(dir.path(), request(dir.path(), "routing")).unwrap();
    assert!(!project.warnings.is_empty());
    assert!(agent::blocking_warnings(&project.warnings).is_empty());
    assert!(
        project.features[0]
            .agent_delivery
            .as_ref()
            .unwrap()
            .matches_source
    );
    let warnings = vec![
        "索引达到 10,000 文件 / 64 MiB 上限，请选择更小目录".to_owned(),
        "未加载标注 broken.json：格式损坏".to_owned(),
    ];
    assert_eq!(agent::blocking_warnings(&warnings).len(), 2);
}

fn request(root: &Path, id: &str) -> Value {
    let inventory = agent::inspect(root).unwrap();
    let previous = inventory["project"]["features"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["id"] == id)
        .map(|f| f["currentDigest"].clone())
        .unwrap_or(Value::Null);
    json!({"schemaVersion":1,"deliveryId":"delivery-1","expectedRevision":inventory["revision"],
        "features":[{"id":id,"expectedDigest":previous,"name":"Routing","description":"Select a route",
            "summary":"Deliver routing implementation","evidence":[{"id":"route-impl","path":"logic.py",
                "symbolId":"logic.py::run","expectedFileHash":inventory["fileHashes"]["logic.py"],
                "role":"implementation","responsibility":"Choose the routing result"}]}]})
}

fn send(root: &Path, value: Value) -> anyhow::Result<ProjectSnapshot> {
    agent::deliver(root, serde_json::from_value(value)?)
}

#[test]
fn delivery_is_separate_atomic_stable_and_preserves_human_confirmation() {
    let dir = fixture();
    let root = dir.path();
    let source = fs::read(root.join("logic.py")).unwrap();
    let created = send(root, request(root, "routing")).unwrap();
    let feature = &created.features[0];
    assert_eq!(feature.id, "routing");
    assert_eq!(feature.review_state, ReviewState::Unread);
    assert!(feature.agent_delivery.as_ref().unwrap().matches_source);
    assert_eq!(feature.anchors[0].role, Some(EvidenceRole::Implementation));
    assert!(!root.join(".reader/features").exists());
    set_review(
        root,
        "routing",
        ReviewState::Verified,
        Some(&feature.current_digest),
    )
    .unwrap();
    let human_path = root.join(".reader/features/routing.json");
    let human_before = fs::read(&human_path).unwrap();
    let retry = send(root, request(root, "routing")).unwrap();
    assert_eq!(retry.features.len(), 1);
    assert_eq!(retry.features[0].review_state, ReviewState::Verified);
    assert_eq!(fs::read(&human_path).unwrap(), human_before);
    let mut update = request(root, "routing");
    update["features"][0]["name"] = json!("Renamed routing");
    let changed = send(root, update).unwrap();
    assert_eq!(changed.features[0].id, "routing");
    assert_eq!(changed.features[0].review_state, ReviewState::NeedsReview);
    assert_eq!(fs::read(&human_path).unwrap(), human_before);
    assert_eq!(fs::read(root.join("logic.py")).unwrap(), source);
}

#[test]
fn stale_tokens_invalid_batch_and_paths_never_replace_a_delivery() {
    let dir = fixture();
    let root = dir.path();
    let stale = request(root, "routing");
    send(root, stale.clone()).unwrap();
    let path = root.join(".reader/agent.json");
    let before = fs::read(&path).unwrap();
    assert!(send(root, stale).is_err());
    for mode in [
        "path",
        "hash",
        "digest",
        "duplicate",
        "symbol",
        "range",
        "role",
    ] {
        let mut update = request(root, "routing");
        match mode {
            "path" => update["features"][0]["evidence"][0]["path"] = json!("../outside.py"),
            "hash" => update["features"][0]["evidence"][0]["expectedFileHash"] = json!("stale"),
            "digest" => update["features"][0]["expectedDigest"] = json!("stale"),
            "duplicate" => {
                let copy = update["features"][0].clone();
                update["features"].as_array_mut().unwrap().push(copy);
            }
            "symbol" => {
                update["features"][0]["evidence"][0]["symbolId"] = json!("logic.py::missing")
            }
            "range" => {
                update["features"][0]["evidence"][0]["startLine"] = json!(1);
                update["features"][0]["evidence"][0]["endLine"] = json!(999);
            }
            _ => update["features"][0]["evidence"][0]["role"] = json!("unknown"),
        }
        assert!(send(root, update).is_err(), "{mode}");
        assert_eq!(fs::read(&path).unwrap(), before, "{mode}");
        assert!(!root.join(".reader/agent.lock").exists());
    }
    let mut inject = request(root, "routing");
    inject["features"][0]["reviewState"] = json!("verified");
    assert!(send(root, inject).is_err());
}

#[test]
fn current_source_is_checked_independently_of_human_review() {
    let dir = fixture();
    let root = dir.path();
    let initial = send(root, request(root, "routing")).unwrap();
    set_review(
        root,
        "routing",
        ReviewState::Verified,
        Some(&initial.features[0].current_digest),
    )
    .unwrap();
    fs::write(root.join("logic.py"), "\n\ndef run():\n    return 1\n").unwrap();
    let moved = open_project(root).unwrap();
    assert!(
        moved.features[0]
            .agent_delivery
            .as_ref()
            .unwrap()
            .matches_source
    );
    assert_eq!(moved.features[0].review_state, ReviewState::Verified);
    fs::write(root.join("logic.py"), "def run():\n    return 2\n").unwrap();
    let changed = open_project(root).unwrap();
    assert!(
        !changed.features[0]
            .agent_delivery
            .as_ref()
            .unwrap()
            .matches_source
    );
    assert_eq!(changed.features[0].review_state, ReviewState::NeedsReview);
    set_review(
        root,
        "routing",
        ReviewState::Verified,
        Some(&changed.features[0].current_digest),
    )
    .unwrap();
    let reviewed = open_project(root).unwrap();
    assert_eq!(reviewed.features[0].review_state, ReviewState::Verified);
    assert!(
        !reviewed.features[0]
            .agent_delivery
            .as_ref()
            .unwrap()
            .matches_source
    );
    fs::remove_file(root.join("logic.py")).unwrap();
    let missing = open_project(root).unwrap();
    assert_eq!(missing.features[0].anchors[0].status, AnchorStatus::Missing);
    assert!(
        !missing.features[0]
            .agent_delivery
            .as_ref()
            .unwrap()
            .matches_source
    );
}

#[test]
fn legacy_files_read_unchanged_and_explicit_adoption_keeps_notes() {
    let dir = fixture();
    let root = dir.path();
    let old = save_feature(
        root,
        FeatureDraft {
            id: None,
            name: "Legacy".into(),
            description: "Old description".into(),
            color: "#aabbcc".into(),
            parent_id: None,
        },
    )
    .unwrap();
    let id = old.features[0].id.clone();
    let bound = bind_anchor(
        root,
        BindRequest {
            feature_id: id.clone(),
            path: "logic.py".into(),
            start_line: 1,
            end_line: 2,
            symbol_id: Some("logic.py::run".into()),
            note: Some("Human explanation".into()),
            anchor_id: None,
        },
    )
    .unwrap();
    let anchor_id = bound.features[0].anchors[0].id.clone();
    set_review(
        root,
        &id,
        ReviewState::Verified,
        Some(&bound.features[0].current_digest),
    )
    .unwrap();
    let path = root.join(format!(".reader/features/{id}.json"));
    let before = fs::read(&path).unwrap();
    let loaded = open_project(root).unwrap();
    assert_eq!(loaded.features[0].review_state, ReviewState::Verified);
    assert!(loaded.features[0].agent_delivery.is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
    let mut adoption = request(root, &id);
    adoption["features"][0]["evidence"][0]["id"] = json!(anchor_id);
    let adopted = send(root, adoption).unwrap();
    assert_eq!(adopted.features[0].anchors[0].note, "Human explanation");
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn sidecar_versions_symlinks_and_active_writers_are_rejected() {
    let dir = fixture();
    let root = dir.path();
    let input = request(root, "routing");
    fs::create_dir(root.join(".reader")).unwrap();
    fs::write(root.join(".reader/agent.lock"), "active").unwrap();
    assert!(send(root, input.clone()).is_err());
    assert!(root.join(".reader/agent.lock").exists());
    fs::remove_file(root.join(".reader/agent.lock")).unwrap();
    fs::write(
        root.join(".reader/agent.json"),
        r#"{"schemaVersion":2,"features":{}}"#,
    )
    .unwrap();
    assert!(open_project(root).is_err());
    assert!(send(root, input.clone()).is_err());
    fs::remove_file(root.join(".reader/agent.json")).unwrap();
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.join("outside.json"), root.join(".reader/agent.json"))
            .unwrap();
        assert!(send(root, input).is_err());
        assert!(!root.join("outside.json").exists());
    }
}

fn guide() -> Value {
    json!({"summary":"Read the system from entry to output", "modules":[
        {"id":"core","name":"Core","responsibility":"Execute routing logic"}],
        "paths":[{"id":"read","name":"Read","description":"Follow the actual evidence", "steps":[
            {"id":"run","title":"Run logic","description":"Read the implementation","moduleId":"core","featureId":"routing","evidenceId":"route-impl"}]}]})
}

#[test]
fn overview_is_atomic_preserved_on_legacy_updates_and_rejects_dangling_links() {
    let dir = fixture();
    let root = dir.path();
    let mut delivery = request(root, "routing");
    delivery["overview"] = guide();
    let p = send(root, delivery).unwrap();
    assert_eq!(p.overview.unwrap().paths[0].steps[0].feature_id, "routing");
    assert!(!root.join(".reader/features").exists());
    let before = fs::read(root.join(".reader/agent.json")).unwrap();
    let mut invalid = request(root, "routing");
    invalid["overview"] = guide();
    invalid["overview"]["paths"][0]["steps"][0]["evidenceId"] = json!("missing");
    assert!(send(root, invalid).is_err());
    assert_eq!(fs::read(root.join(".reader/agent.json")).unwrap(), before);
    let mut invalid = request(root, "routing");
    invalid["features"][0]["evidence"][0]["id"] = json!("replacement");
    assert!(send(root, invalid).is_err());
    assert_eq!(fs::read(root.join(".reader/agent.json")).unwrap(), before);
    // Old callers omit overview; it is retained rather than removed.
    assert!(send(root, request(root, "routing"))
        .unwrap()
        .overview
        .is_some());
}

#[test]
fn overview_rejects_duplicate_ids_unknown_modules_and_unsupported_fields() {
    let dir = fixture();
    for kind in ["duplicate", "module", "review", "empty"] {
        let mut d = request(dir.path(), "routing");
        d["overview"] = guide();
        match kind {
            "duplicate" => {
                let step = d["overview"]["paths"][0]["steps"][0].clone();
                d["overview"]["paths"][0]["steps"]
                    .as_array_mut()
                    .unwrap()
                    .push(step);
            }
            "module" => d["overview"]["paths"][0]["steps"][0]["moduleId"] = json!("missing"),
            "review" => d["overview"]["reviewState"] = json!("verified"),
            _ => d["overview"]["summary"] = json!(" "),
        }
        assert!(send(dir.path(), d).is_err(), "{kind}");
        assert!(!dir.path().join(".reader/agent.json").exists());
    }
}

#[test]
fn malformed_stored_guide_reports_warning_without_hiding_source() {
    let dir = fixture();
    let mut d = request(dir.path(), "routing");
    d["overview"] = guide();
    send(dir.path(), d).unwrap();
    let path = dir.path().join(".reader/agent.json");
    let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    stored["overview"]["paths"][0]["steps"][0]["featureId"] = json!("missing");
    fs::write(path, serde_json::to_vec(&stored).unwrap()).unwrap();
    let p = open_project(dir.path()).unwrap();
    assert!(p.overview.is_none());
    assert!(!p.files.is_empty());
    assert!(!agent::blocking_warnings(&p.warnings).is_empty());
}
