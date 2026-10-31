use reader_core::collaboration::{self, Host};
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
fn git(root: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .unwrap()
        .success());
}
fn project() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q"]);
    git(dir.path(), &["config", "user.name", "Fixture"]);
    git(
        dir.path(),
        &["config", "user.email", "fixture@example.invalid"],
    );
    fs::write(dir.path().join("a.py"), "def run():\n    return 1\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-qm", "baseline"]);
    dir
}
#[test]
fn version_manifest_accepts_crlf_but_rejects_uncommitted_and_untracked_code() {
    let p = project();
    let before = collaboration::manifest(p.path()).unwrap();
    fs::write(p.path().join("a.py"), "def run():\r\n    return 1\r\n").unwrap();
    let after = collaboration::manifest(p.path()).unwrap();
    assert_eq!(before.version, after.version);
    fs::write(p.path().join("a.py"), "def run():\n    return 2\n").unwrap();
    assert!(collaboration::manifest(p.path())
        .unwrap_err()
        .to_string()
        .contains("尚未提交"));
    fs::write(p.path().join("a.py"), "def run():\n    return 1\n").unwrap();
    fs::write(p.path().join("new.py"), "value=1\n").unwrap();
    assert!(collaboration::manifest(p.path())
        .unwrap_err()
        .to_string()
        .contains("未提交文件"));
    fs::remove_file(p.path().join("new.py")).unwrap();
    fs::remove_file(p.path().join("a.py")).unwrap();
    assert!(collaboration::manifest(p.path()).is_err());
}
#[test]
fn credentials_stay_outside_project_shared_drafts_recover_and_old_comments_remain_exportable() {
    let a = project();
    let b = tempfile::tempdir().unwrap();
    assert!(Command::new("git")
        .args([
            "clone",
            "-q",
            a.path().to_str().unwrap(),
            b.path().join("project").to_str().unwrap()
        ])
        .status()
        .unwrap()
        .success());
    let b = b.path().join("project");
    let private_a = tempfile::tempdir().unwrap();
    let private_b = tempfile::tempdir().unwrap();
    let hostdata = tempfile::tempdir().unwrap();
    let host = Host::start(hostdata.path(), Some(std::net::Ipv4Addr::LOCALHOST)).unwrap();
    let act = |root: &Path, private: &Path, action: &str, data: Value| {
        collaboration::action(root, private, action, data)
    };
    let _created = act(
        a.path(),
        private_a.path(),
        "create",
        json!({"server":host.server,"createKey":host.key,"name":"主持人","title":"本机共读"}),
    )
    .unwrap();
    let invitation = act(a.path(), private_a.path(), "invitation", json!({})).unwrap();
    let guest = act(
        &b,
        private_b.path(),
        "join",
        json!({"invitation":invitation,"name":"参与者"}),
    )
    .unwrap();
    let guest_id = guest["me"].clone();
    let room = act(a.path(), private_a.path(), "read", json!({})).unwrap();
    let room = act(
        a.path(),
        private_a.path(),
        "member",
        json!({"expectedRevision":room["revision"],"memberId":guest_id,"status":"active"}),
    )
    .unwrap();
    let room=act(a.path(),private_a.path(),"comment",json!({"expectedRevision":room["revision"],"comment":{"id":"shared","body":"请补充解释","featureId":null,"scope":{"path":"a.py","startLine":1,"endLine":2},"parentId":null}})).unwrap();
    let task = json!({"id":"task","title":"解释返回值","goal":"说明返回值作用","acceptance":"阅读者能理解","boundary":"不改接口","commentIds":["shared"],"featureIds":[],"scopes":[],"contracts":[],"ownerId":guest_id,"dependsOn":[],"couplingNote":"仅此任务"});
    act(
        a.path(),
        private_a.path(),
        "task",
        json!({"expectedRevision":room["revision"],"task":task}),
    )
    .unwrap();
    let online = act(&b, private_b.path(), "read", json!({})).unwrap();
    assert_eq!(online["comments"].as_array().unwrap().len(), 1);
    assert!(!a.path().join(".reader").exists());
    assert!(!b.join(".reader").exists());
    let old_hash = reader_core::read_file(&b, "a.py").unwrap().hash;
    let bad = act(
        &b,
        private_b.path(),
        "comment",
        json!({"expectedRevision":online["revision"],"localFileHash":"wrong","comment":{"id":"stale","body":"来自旧批注","featureId":null,"scope":{"path":"a.py","startLine":1,"endLine":2},"parentId":null}}),
    );
    assert!(bad.is_err());
    assert!(!old_hash.is_empty());
    fs::write(b.join("a.py"), "def run():\n    return 999\n").unwrap();
    assert!(act(&b,private_b.path(),"comment",json!({"expectedRevision":online["revision"],"comment":{"id":"changed","body":"不能混入新代码","featureId":null,"scope":null,"parentId":null}})).is_err());
    drop(host);
    let cached = act(&b, private_b.path(), "read", json!({})).unwrap();
    assert_eq!(cached["offline"], true);
    let export = act(&b, private_b.path(), "export", json!({"ownerId":guest_id})).unwrap();
    assert!(export.as_str().unwrap().contains("解释返回值"));
    assert!(export.as_str().unwrap().contains("请补充解释"));
    act(&b, private_b.path(), "leave", json!({})).unwrap();
    let history = act(&b, private_b.path(), "history", json!({})).unwrap();
    assert_eq!(history[0]["id"], room["id"]);
    let resumed = act(&b, private_b.path(), "resume", json!({"id":room["id"]})).unwrap();
    assert_eq!(resumed["offline"], true);
}
#[test]
fn positive_python_call_hints_connect_features_without_claiming_full_independence() {
    let p = project();
    fs::write(
        p.path().join("a.py"),
        "def helper():\n    return 1\n\ndef run():\n    return helper()\n",
    )
    .unwrap();
    git(p.path(), &["add", "."]);
    git(p.path(), &["commit", "-qm", "calls"]);
    let snap = reader_core::open_project(p.path()).unwrap();
    let mut features = Vec::new();
    for id in ["helper", "run"] {
        let feature = reader_core::save_feature(
            p.path(),
            reader_core::FeatureDraft {
                id: None,
                name: id.into(),
                description: id.into(),
                color: "#abcdef".into(),
                parent_id: None,
            },
        )
        .unwrap()
        .features
        .into_iter()
        .find(|f| f.name == id)
        .unwrap();
        let symbol = snap.symbols.iter().find(|s| s.name == id).unwrap();
        reader_core::bind_anchor(
            p.path(),
            reader_core::BindRequest {
                feature_id: feature.id.clone(),
                symbol_id: Some(symbol.id.clone()),
                path: "a.py".into(),
                start_line: symbol.range.start_line,
                end_line: symbol.range.end_line,
                note: None,
                anchor_id: None,
            },
        )
        .unwrap();
        features.push(feature);
    }
    let m = collaboration::manifest(p.path()).unwrap();
    let run = m.features.iter().find(|f| f.name == "run").unwrap();
    let helper = m.features.iter().find(|f| f.name == "helper").unwrap();
    assert!(run.dependencies.contains(&helper.id));
}
