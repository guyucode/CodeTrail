use reader_collab::{service::Service, *};
use serde_json::{json, Value};
fn manifest() -> Manifest {
    Manifest {
        version: Version {
            commit: "a".repeat(40),
            source_digest: "b".repeat(64),
            association_digest: "c".repeat(64),
        },
        features: vec![FeatureScope {
            id: "login".into(),
            name: "登录".into(),
            evidence: vec![Scope {
                path: "src/login.py".into(),
                start_line: 1,
                end_line: 20,
            }],
            modules: vec!["auth".into()],
            dependencies: vec![],
        }],
    }
}
fn req(action: &str, id: &str, data: Value) -> Request {
    Request {
        action: action.into(),
        session_id: id.into(),
        data,
    }
}
struct Fixture {
    dir: tempfile::TempDir,
    service: Service,
    admin: String,
    host: String,
    guest: String,
    invite: String,
    room: Value,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let admin = secret();
        let host = secret();
        let invite = secret();
        let service = Service::open(dir.path(), &admin).unwrap();
        let room=service.handle(&admin,req("create","",json!({"id":"room","title":"登录评审","name":"发起人","ownerToken":host,"inviteToken":invite,"manifest":manifest()}))).unwrap();
        Self {
            dir,
            service,
            admin,
            host,
            guest: secret(),
            invite,
            room,
        }
    }
    fn call(&mut self, token: &str, action: &str, mut data: Value) -> anyhow::Result<Value> {
        if data.is_null() {
            data = json!({});
        }
        data["expectedRevision"] = self.room["revision"].clone();
        let result = self.service.handle(token, req(action, "room", data));
        if result.is_ok() {
            self.room = self
                .service
                .handle(&self.host, req("read", "room", json!({})))
                .unwrap();
        }
        result
    }
    fn join(&mut self) -> Value {
        self.call(&self.guest.clone(),"join",json!({"version":manifest().version,"memberToken":self.guest,"inviteToken":self.invite,"name":"成员"})).unwrap()
    }
    fn approve(&mut self) -> String {
        let pending = self.join();
        let id = pending["me"].as_str().unwrap().to_owned();
        self.call(
            &self.host.clone(),
            "member",
            json!({"memberId":id,"status":"active"}),
        )
        .unwrap();
        id
    }
    fn comment(&mut self, id: &str) -> Value {
        self.call(&self.host.clone(),"comment",json!({"comment":{"id":id,"body":"请完善登录过期提示","featureId":"login","scope":null,"parentId":null}})).unwrap()
    }
    fn task(&self, id: &str, owner: Value) -> Value {
        json!({"id":id,"title":id,"goal":"完善登录","acceptance":"有明确过期提示","boundary":"保持请求接口","commentIds":["comment"],"featureIds":[],"scopes":[],"contracts":[],"ownerId":owner,"dependsOn":[],"couplingNote":"共享登录实现保持兼容"})
    }
}
#[test]
fn membership_is_explicit_version_bound_and_revocable() {
    let mut f = Fixture::new();
    f.comment("comment");
    assert!(f
        .service
        .handle("wrong", req("read", "room", json!({})))
        .is_err());
    let mut wrong = manifest().version;
    wrong.source_digest = "d".repeat(64);
    assert!(f
        .call(
            "",
            "join",
            json!({"version":wrong,"memberToken":secret(),"inviteToken":f.invite,"name":"不同版本"})
        )
        .is_err());
    let pending = f.join();
    assert_eq!(pending["comments"], json!([]));
    assert_eq!(pending["members"].as_array().unwrap().len(), 1);
    assert!(f.call(&f.guest.clone(),"comment",json!({"comment":{"id":"blocked","body":"不能发","featureId":null,"scope":null,"parentId":null}})).is_err());
    let id = pending["me"].as_str().unwrap().to_owned();
    f.call(
        &f.host.clone(),
        "member",
        json!({"memberId":id,"status":"active"}),
    )
    .unwrap();
    let view = f.call(&f.guest.clone(), "read", json!({})).unwrap();
    assert_eq!(view["comments"].as_array().unwrap().len(), 1);
    assert!(!view.to_string().contains("tokenHash"));
    assert!(!view.to_string().contains(&f.invite));
    assert!(f.call(&f.guest.clone(), "close", json!({})).is_err());
    f.call(
        &f.host.clone(),
        "member",
        json!({"memberId":id,"status":"removed"}),
    )
    .unwrap();
    assert!(f.call(&f.guest.clone(), "read", json!({})).is_err());
}
#[test]
fn retries_are_idempotent_concurrent_writes_do_not_silently_overwrite() {
    let mut f = Fixture::new();
    let rev = f.room["revision"].clone();
    f.comment("comment");
    let input = json!({"comment":{"id":"comment","body":"请完善登录过期提示","featureId":"login","scope":null,"parentId":null},"expectedRevision":rev});
    f.service
        .handle(&f.host, req("comment", "room", input))
        .unwrap();
    assert_eq!(f.room["comments"].as_array().unwrap().len(), 1);
    let mut stale = json!({"comment":{"id":"other","body":"并发意见","featureId":null,"scope":null,"parentId":null}});
    stale["expectedRevision"] = rev;
    assert!(f
        .service
        .handle(&f.host, req("comment", "room", stale))
        .is_err());
    assert_eq!(
        f.service
            .handle(&f.host, req("read", "room", json!({})))
            .unwrap()["comments"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}
#[test]
fn task_scope_is_derived_from_comments_and_overlap_blocks_uncoordinated_split() {
    let mut f = Fixture::new();
    let guest = f.approve();
    f.comment("comment");
    let host = f.room["me"].clone();
    let one = f.task("one", host);
    f.call(&f.host.clone(), "task", json!({"task":one}))
        .unwrap();
    assert_eq!(f.room["tasks"][0]["input"]["featureIds"], json!(["login"]));
    let two = f.task("two", json!(guest));
    let error = f
        .call(&f.host.clone(), "task", json!({"task":two}))
        .unwrap_err();
    assert!(error.to_string().contains("重叠代码"));
    assert_eq!(f.room["tasks"].as_array().unwrap().len(), 1);
    let mut two = f.task("two", json!(guest));
    two["dependsOn"] = json!(["one"]);
    f.call(&f.host.clone(), "task", json!({"task":two}))
        .unwrap();
    assert!(f
        .call(
            &f.guest.clone(),
            "taskStatus",
            json!({"taskId":"two","status":"accepted"})
        )
        .is_err());
    f.call(
        &f.host.clone(),
        "taskStatus",
        json!({"taskId":"one","status":"accepted"}),
    )
    .unwrap();
    f.call(
        &f.host.clone(),
        "taskStatus",
        json!({"taskId":"one","status":"needsReview","resultCommit":"e".repeat(40)}),
    )
    .unwrap();
    assert!(f.call(&f.guest.clone(),"taskStatus",json!({"taskId":"one","status":"reviewed","resultCommit":"e".repeat(40),"reviewNote":"我替人确认"})).is_err());
    f.call(&f.host.clone(),"taskStatus",json!({"taskId":"one","status":"reviewed","resultCommit":"e".repeat(40),"reviewNote":"人工查看过过期分支及测试结果"})).unwrap();
    f.call(
        &f.guest.clone(),
        "taskStatus",
        json!({"taskId":"two","status":"accepted"}),
    )
    .unwrap();
    assert!(f
        .call(&f.host.clone(), "task", json!({"task":one}))
        .is_err());
    let exported = f
        .call(&f.guest.clone(), "export", json!({"ownerId":guest}))
        .unwrap();
    let text = exported.as_str().unwrap();
    assert!(text.contains("## two"));
    assert!(!text.contains("## one"));
    assert!(text.contains("请完善登录过期提示"));
    assert!(text.contains("保持请求接口"));
    assert!(text.contains("先处理前置任务"));
}
#[test]
fn cycles_are_rejected_even_for_one_owner_and_invites_rotate() {
    let mut f = Fixture::new();
    f.comment("comment");
    let host = f.room["me"].clone();
    let mut a = f.task("a", host.clone());
    let mut b = f.task("b", host);
    f.call(&f.host.clone(), "task", json!({"task":a})).unwrap();
    b["dependsOn"] = json!(["a"]);
    f.call(&f.host.clone(), "task", json!({"task":b})).unwrap();
    a["dependsOn"] = json!(["b"]);
    assert!(f.call(&f.host.clone(), "task", json!({"task":a})).is_err());
    f.call(
        &f.host.clone(),
        "rotateInvite",
        json!({"inviteToken":secret()}),
    )
    .unwrap();
    assert!(f.call("","join",json!({"version":manifest().version,"memberToken":f.guest,"inviteToken":f.invite,"name":"旧邀请"})).is_err());
    f.call(&f.host.clone(), "close", json!({})).unwrap();
    assert!(f.call(&f.host.clone(),"comment",json!({"comment":{"id":"closed","body":"不能追加","featureId":null,"scope":null,"parentId":null}})).is_err());
    assert!(f
        .call(&f.host.clone(), "export", json!({"ownerId":f.room["me"]}))
        .is_ok());
}
#[test]
fn restart_preserves_discussion_and_exclusive_store_lock() {
    let f = Fixture::new();
    assert!(Service::open(f.dir.path(), &f.admin).is_err());
    let Fixture {
        dir,
        service,
        admin,
        host,
        ..
    } = f;
    drop(service);
    let restored = Service::open(dir.path(), &admin).unwrap();
    let room = restored
        .handle(&host, req("read", "room", json!({})))
        .unwrap();
    assert_eq!(room["title"], "登录评审");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(dir.path().join("collaboration.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
#[test]
fn lan_client_rejects_public_endpoints_and_scope_escape() {
    for bad in [
        "http://8.8.8.8:8787",
        "https://example.com",
        "http://user:secret@192.168.1.2",
        "file:///tmp/test",
        "http://192.168.1.2?token=secret",
        "http://example.com",
    ] {
        assert!(client::endpoint(bad).is_err(), "{bad}");
    }
    assert!(client::endpoint("http://192.168.1.2:8787").is_ok());
    assert!(client::endpoint("http://127.0.0.1:8787").is_ok());
    for path in [
        "../a.py",
        "/a.py",
        "C:/a.py",
        "src/.git/config",
        "src\\a.py",
    ] {
        assert!(!tasks::scope_valid(&Scope {
            path: path.into(),
            start_line: 1,
            end_line: 2
        }));
    }
}
#[test]
fn actual_http_host_supports_two_clients_without_a_remote_server() {
    let dir = tempfile::tempdir().unwrap();
    let host = server::Host::start(dir.path(), Some(std::net::Ipv4Addr::LOCALHOST)).unwrap();
    let token = secret();
    let invite = secret();
    let room=client::call(&host.server,&host.key,&req("create","",json!({"id":"network-room","title":"本机双客户端","name":"主持人","ownerToken":token,"inviteToken":invite,"manifest":manifest()}))).unwrap();
    let guest = secret();
    let pending=client::call(&host.server,"",&req("join","network-room",json!({"version":manifest().version,"memberToken":guest,"inviteToken":invite,"name":"参与者"}))).unwrap();
    assert_eq!(pending["members"][0]["status"], "pending");
    let latest = client::call(
        &host.server,
        &token,
        &req("read", "network-room", json!({})),
    )
    .unwrap();
    assert!(latest["revision"].as_u64() > room["revision"].as_u64());
    client::call(&host.server,&token,&req("member","network-room",json!({"memberId":pending["me"],"status":"active","expectedRevision":latest["revision"]}))).unwrap();
    let joined = client::call(
        &host.server,
        &guest,
        &req("read", "network-room", json!({})),
    )
    .unwrap();
    assert_eq!(joined["members"].as_array().unwrap().len(), 2);
    drop(host);
    assert!(client::call(
        "http://127.0.0.1:1",
        &guest,
        &req("read", "network-room", json!({}))
    )
    .is_err());
}

#[test]
fn merging_tasks_preserves_sources_remaps_dependencies_and_exports_once() {
    let mut f = Fixture::new();
    f.comment("comment");
    let host = f.room["me"].clone();
    for id in ["a", "b", "downstream"] {
        let mut task = f.task(id, host.clone());
        if id == "downstream" {
            task["dependsOn"] = json!(["b"]);
        }
        f.call(&f.host.clone(), "task", json!({"task":task}))
            .unwrap();
    }
    f.call(
        &f.host.clone(),
        "mergeTasks",
        json!({"targetId":"a","sourceIds":["b"]}),
    )
    .unwrap();
    assert_eq!(f.room["tasks"][1]["status"], "merged");
    assert_eq!(f.room["tasks"][1]["mergedInto"], "a");
    assert_eq!(f.room["tasks"][2]["input"]["dependsOn"], json!(["a"]));
    assert!(f.room["tasks"][0]["input"]["goal"]
        .as_str()
        .unwrap()
        .contains("合并「b」"));
    let exported = f
        .call(&f.host.clone(), "export", json!({"ownerId":host}))
        .unwrap();
    let text = exported.as_str().unwrap();
    assert!(text.contains("## a\n"));
    assert!(!text.contains("## b\n"));
    assert!(text.contains("src/login.py"));
    assert!(f
        .call(
            &f.host.clone(),
            "taskStatus",
            json!({"taskId":"b","status":"accepted"})
        )
        .is_err());
}

#[test]
fn owners_accept_their_own_tasks_and_review_is_bound_to_submitted_commit() {
    let mut f = Fixture::new();
    let guest = f.approve();
    f.comment("comment");
    let task = f.task("owned", json!(guest));
    f.call(&f.host.clone(), "task", json!({"task":task}))
        .unwrap();
    assert!(f
        .call(
            &f.host.clone(),
            "taskStatus",
            json!({"taskId":"owned","status":"accepted"})
        )
        .is_err());
    f.call(
        &f.guest.clone(),
        "taskStatus",
        json!({"taskId":"owned","status":"accepted"}),
    )
    .unwrap();
    f.call(
        &f.guest.clone(),
        "taskStatus",
        json!({"taskId":"owned","status":"needsReview","resultCommit":"e".repeat(40)}),
    )
    .unwrap();
    assert!(f.call(&f.host.clone(),"taskStatus",json!({"taskId":"owned","status":"reviewed","resultCommit":"f".repeat(40),"reviewNote":"不能悄悄换版本"})).is_err());
    f.call(&f.host.clone(),"taskStatus",json!({"taskId":"owned","status":"reviewed","resultCommit":"e".repeat(40),"reviewNote":"已核对本次交付"})).unwrap();
}

#[test]
fn default_invitation_uses_available_lan_interface_even_with_a_proxy_route() {
    let available = server::local_addresses();
    let dir = tempfile::tempdir().unwrap();
    let host = server::Host::start(dir.path(), None).unwrap();
    let endpoint = client::endpoint(&host.server).unwrap();
    if !available.is_empty() {
        assert_ne!(endpoint.host_str(), Some("127.0.0.1"));
        assert!(available
            .iter()
            .any(|ip| Some(ip.as_str()) == endpoint.host_str()));
    }
    let room=client::call(&host.server,&host.key,&req("create","",json!({"id":"lan-address-regression","title":"网卡识别回归","name":"测试主持人","ownerToken":secret(),"inviteToken":secret(),"manifest":manifest()}))).unwrap();
    assert_eq!(room["title"], "网卡识别回归");
}
