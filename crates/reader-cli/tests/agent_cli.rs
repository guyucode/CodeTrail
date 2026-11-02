use serde_json::{json, Value};
use std::{
    fs,
    io::Write,
    path::PathBuf,
    process::{Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static REPO_COUNTER: AtomicU64 = AtomicU64::new(0);

struct Repo(PathBuf);
impl Repo {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "codetrail-cli-{}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            REPO_COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("logic.py"), "def run():\n    return 1\n").unwrap();
        Self(path)
    }
    fn call(&self, action: &str, payload: Option<&Value>) -> (bool, Value) {
        let mut child = Command::new(env!("CARGO_BIN_EXE_reader-cli"))
            .args(["agent", action, self.0.to_str().unwrap()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        if let Some(value) = payload {
            child
                .stdin
                .take()
                .unwrap()
                .write_all(serde_json::to_string(value).unwrap().as_bytes())
                .unwrap();
        } else {
            drop(child.stdin.take());
        }
        let out = child.wait_with_output().unwrap();
        (
            out.status.success(),
            serde_json::from_slice(&out.stdout).expect("CLI must emit exactly one JSON document"),
        )
    }
}
impl Drop for Repo {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn cli_inspects_delivers_checks_and_rejects_review_injection() {
    let repo = Repo::new();
    let (ok, inventory) = repo.call("inspect", None);
    assert!(ok);
    let mut request = json!({"schemaVersion":1,"deliveryId":"cli-1","expectedRevision":inventory["revision"],"features":[{
        "id":"routing","expectedDigest":null,"name":"Routing","description":"Choose a route","summary":"Initial implementation",
        "evidence":[{"id":"impl","path":"logic.py","symbolId":"logic.py::run","expectedFileHash":inventory["fileHashes"]["logic.py"],
            "role":"implementation","responsibility":"Choose route"}]}]});
    request["features"][0]["reviewState"] = json!("verified");
    assert!(!repo.call("deliver", Some(&request)).0);
    request["features"][0]
        .as_object_mut()
        .unwrap()
        .remove("reviewState");
    assert!(repo.call("deliver", Some(&request)).0);
    assert!(repo.call("check", None).0);
    fs::write(repo.0.join("logic.py"), "def run():\n    return 2\n").unwrap();
    let (ok, report) = repo.call("check", None);
    assert!(!ok);
    assert_eq!(report["ok"], false);
    assert!(!repo.0.join(".reader/features").exists());
}

impl Repo {
    fn git(&self, args: &[&str]) {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&self.0)
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(args)
            .output()
            .unwrap()
            .status
            .success());
    }
    fn baseline(&self) {
        self.git(&["init", "-q"]);
        self.git(&["add", "."]);
        self.git(&[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "baseline",
        ]);
    }
    fn coverage(&self) -> Value {
        let out = Command::new(env!("CARGO_BIN_EXE_reader-cli"))
            .args(["agent", "coverage", self.0.to_str().unwrap(), "HEAD"])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stdout)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn deliver_logic(&self) {
        let (_, i) = self.call("inspect", None);
        let previous = i["project"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["id"] == "routing");
        let request = json!({"schemaVersion":1,"deliveryId":"workflow-test","expectedRevision":i["revision"],
            "features":[{"id":"routing","expectedDigest":previous.map(|f|f["currentDigest"].clone()),
            "name":"Routing","description":"Run logic","summary":"Test delivery",
            "evidence":[{"id":"impl","path":"logic.py","symbolId":"logic.py::run",
            "expectedFileHash":i["fileHashes"]["logic.py"],"role":"implementation","responsibility":"Run logic"}]}]});
        assert!(self.call("deliver", Some(&request)).0);
    }
}

#[test]
fn draft_keeps_identity_and_coverage_does_not_hide_new_unbound_functions() {
    let repo = Repo::new();
    repo.deliver_logic();
    repo.baseline();
    fs::write(
        repo.0.join("logic.py"),
        "def run():\n    return 2\n\ndef missing():\n    return 3\n",
    )
    .unwrap();
    let (ok, draft) = repo.call("draft", None);
    assert!(ok);
    assert_eq!(draft["features"][0]["id"], "routing");
    assert_eq!(draft["features"][0]["evidence"][0]["id"], "impl");
    repo.deliver_logic();
    let c = repo.coverage();
    assert_eq!(c["files"][0]["featureIds"][0], "routing");
    assert!(c["files"][0]["uncoveredLines"]
        .as_array()
        .unwrap()
        .contains(&json!(4)));
    assert_eq!(c["files"][0]["needsExplanation"], true);
}

#[test]
fn validate_requires_current_explanations_runs_tests_and_detects_source_mutation() {
    let repo = Repo::new();
    repo.baseline();
    fs::write(repo.0.join("notes.md"), "Documentation change\n").unwrap();
    let mut plan = json!({"schemaVersion":1,"base":"HEAD","unassociated":[],
        "checks":[{"program":"git","args":["--version"]}]});
    assert!(!repo.call("validate", Some(&plan)).0);
    let c = repo.coverage();
    plan["unassociated"] = json!([{"path":"notes.md","changeDigest":c["files"][0]["changeDigest"],"reason":"Documentation only"}]);
    assert!(repo.call("validate", Some(&plan)).0);
    plan["checks"][0]["args"] = json!(["not-a-real-command"]);
    let (ok, report) = repo.call("validate", Some(&plan));
    assert!(!ok);
    assert_eq!(report["checks"][0]["ok"], false);
    plan["checks"] = json!([]);
    assert!(!repo.call("validate", Some(&plan)).0);
    plan["checks"] = json!([{"program":"git","args":["--version"]}]);
    fs::write(repo.0.join("notes.md"), "Different documentation\n").unwrap();
    assert!(!repo.call("validate", Some(&plan)).0);
    let c = repo.coverage();
    plan["unassociated"][0]["changeDigest"] = c["files"][0]["changeDigest"].clone();
    // A check that writes tracked source must invalidate the final gate.
    plan["checks"] =
        json!([{"program":"sh","args":["-c","printf 'def run():\\n    return 9\\n' > logic.py"]}]);
    let (ok, report) = repo.call("validate", Some(&plan));
    assert!(!ok);
    assert_eq!(report["sourceAndMetadataStable"], false);
    assert!(!repo.0.join(".reader/features").exists());
}

#[test]
fn unchanged_binary_assets_do_not_block_but_changed_assets_do() {
    let repo = Repo::new();
    fs::write(repo.0.join("icon.png"), [0, 1, 2]).unwrap();
    repo.baseline();
    assert_eq!(repo.coverage()["files"], json!([]));
    fs::write(repo.0.join("icon.png"), [0, 1, 3]).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_reader-cli"))
        .args(["agent", "coverage", repo.0.to_str().unwrap(), "HEAD"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    let report: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(report["error"].as_str().unwrap().contains("icon.png"));
}

#[test]
fn draft_preserves_overview_and_check_rejects_damaged_guide_links() {
    let repo = Repo::new();
    repo.deliver_logic();
    let (_, mut draft) = repo.call("draft", None);
    draft["overview"] = json!({"summary":"Read routing", "modules":[{"id":"core","name":"Core","responsibility":"Run logic"}],
        "paths":[{"id":"read","name":"Read","description":"Read the entry", "steps":[{"id":"entry","title":"Run","description":"Read actual code","moduleId":"core","featureId":"routing","evidenceId":"impl"}]}]});
    assert!(repo.call("deliver", Some(&draft)).0);
    let (ok, next) = repo.call("draft", None);
    assert!(ok);
    assert_eq!(next["overview"], draft["overview"]);
    let path = repo.0.join(".reader/agent.json");
    let mut stored: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    stored["overview"]["paths"][0]["steps"][0]["featureId"] = json!("missing");
    fs::write(path, serde_json::to_vec(&stored).unwrap()).unwrap();
    assert!(!repo.call("check", None).0);
}

#[test]
fn cli_delivers_c_and_cpp_symbols_and_refreshes_moved_evidence() {
    let repo = Repo::new();
    fs::write(
        repo.0.join("sum.c"),
        "int add(int a, int b) { return a+b; }\n",
    )
    .unwrap();
    let cpp = "namespace api { class Engine { public: int run(int x) { return x; } }; }\n";
    fs::write(repo.0.join("engine.hpp"), cpp).unwrap();
    let (ok, inventory) = repo.call("inspect", None);
    assert!(ok);
    let evidence: Vec<_> = ["add", "api::Engine::run"].iter().enumerate().map(|(i, name)| {
        let symbol = inventory["project"]["symbols"].as_array().unwrap().iter().find(|s| s["qualifiedName"] == *name).unwrap();
        let path = symbol["range"]["file"].as_str().unwrap();
        json!({"id":format!("impl-{i}"),"path":path,"symbolId":symbol["id"],"expectedFileHash":inventory["fileHashes"][path],"role":"implementation","responsibility":"Compute the documented fixture result"})
    }).collect();
    let payload = json!({"schemaVersion":1,"deliveryId":"c-family-fixture","expectedRevision":inventory["revision"],"features":[{"id":"native-compute","name":"Native compute","description":"Read C and C++ entry points","summary":"Synthetic integration fixture","expectedDigest":null,"evidence":evidence}]});
    assert!(repo.call("deliver", Some(&payload)).0);
    assert!(repo.call("check", None).0);
    fs::write(
        repo.0.join("engine.hpp"),
        format!(
            "// moved by fixture\n{}",
            cpp.replace("return x;", "return x + 1;")
        ),
    )
    .unwrap();
    assert!(!repo.call("check", None).0);
    let (ok, mut draft) = repo.call("draft", None);
    assert!(ok);
    draft["deliveryId"] = json!("c-family-fixture-moved");
    draft["features"][0]["summary"] = json!("Refresh actual moved C++ source evidence");
    assert!(repo.call("deliver", Some(&draft)).0);
    assert!(repo.call("check", None).0);
}

#[test]
fn validate_records_real_exit_codes_without_overwriting_legacy_or_human_data() {
    let repo = Repo::new();
    repo.deliver_logic();
    repo.baseline();
    let feedback = br#"[{"id":"synthetic-feedback","body":"keep this isolated test note"}]"#;
    fs::write(repo.0.join(".reader/annotations.json"), feedback).unwrap();
    let program = env!("CARGO_BIN_EXE_reader-cli");
    let mut plan = json!({"schemaVersion":1,"base":"HEAD","unassociated":[],"checks":[{"program":program,"args":["agent","check","."]}],"record":{"goal":"Check actual delivered fixture","path":"logic.py","startLine":1,"endLine":2}});
    let (ok, result) = repo.call("validate", Some(&plan));
    assert!(ok, "{result}");
    let id = result["verificationRunId"].as_str().unwrap();
    let view = reader_core::trace::read(&repo.0, id).unwrap();
    assert_eq!(view.run.status, "completed");
    assert_eq!(view.run.verification.unwrap().verdict, "matched");
    assert_eq!(view.steps[0].record.observation.outputs[0].value, "0");
    let before = fs::read(repo.0.join(".reader/runs.json")).unwrap();
    let mut legacy = plan.clone();
    legacy.as_object_mut().unwrap().remove("record");
    assert!(repo.call("validate", Some(&legacy)).0);
    assert_eq!(before, fs::read(repo.0.join(".reader/runs.json")).unwrap());
    plan["checks"][0]["args"] = json!(["unknown-command"]);
    let (ok, result) = repo.call("validate", Some(&plan));
    assert!(!ok);
    let view =
        reader_core::trace::read(&repo.0, result["verificationRunId"].as_str().unwrap()).unwrap();
    assert_eq!(view.run.status, "failed");
    assert_eq!(view.run.verification.unwrap().verdict, "different");
    assert_eq!(view.steps[0].record.observation.outputs[0].value, "1");
    plan["record"]["path"] = json!(".reader/agent.json");
    let before = fs::read(repo.0.join(".reader/runs.json")).unwrap();
    assert!(!repo.call("validate", Some(&plan)).0);
    assert_eq!(before, fs::read(repo.0.join(".reader/runs.json")).unwrap());
    assert_eq!(
        feedback.as_slice(),
        fs::read(repo.0.join(".reader/annotations.json")).unwrap()
    );
}

#[test]
fn verification_cannot_pass_when_check_commands_change_target_source() {
    let repo = Repo::new();
    repo.deliver_logic();
    repo.baseline();
    let plan = json!({"schemaVersion":1,"base":"HEAD","unassociated":[],"checks":[{"program":"git","args":["mv","logic.py","moved.py"]}],"record":{"goal":"Keep source stable","path":"logic.py","startLine":1,"endLine":2}});
    let (ok, result) = repo.call("validate", Some(&plan));
    assert!(!ok, "{result}");
    assert_eq!(result["sourceAndMetadataStable"], false);
    let view =
        reader_core::trace::read(&repo.0, result["verificationRunId"].as_str().unwrap()).unwrap();
    assert_eq!(view.run.status, "failed");
    assert_eq!(view.run.verification.unwrap().verdict, "different");
    assert!(!result["verificationErrors"].as_array().unwrap().is_empty());
}
