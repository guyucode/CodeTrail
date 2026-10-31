//! Desktop boundary: explicit sharing, local credentials, version-checked writes.
use crate::*;
pub use reader_collab::server::{local_addresses, Host};
use reader_collab::{client, FeatureScope, Manifest, Request, Scope, Version};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

fn git_command(root: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("--no-optional-locks")
        .arg("-c")
        .arg("core.fsmonitor=false")
        .arg("-C")
        .arg(root);
    c
}
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}
/// Compare captured readable files with HEAD blobs, without external Git filters.
/// CRLF/LF are equivalent for sharing; local raw hashes still guard navigation.
pub fn manifest(root: &Path) -> Result<Manifest> {
    let root = canonical_root(root)?;
    let top = git_command(&root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("局域网共读需要 Git，请先安装并重新打开应用")?;
    ensure!(top.status.success(), "共读需要已有提交的 Git 仓库");
    ensure!(
        Path::new(String::from_utf8_lossy(&top.stdout).trim()).canonicalize()? == root,
        "共读请打开 Git 仓库根目录"
    );
    let scan = scan_repository(&root)?;
    ensure!(
        scan.warnings.iter().all(|w| w.contains("二进制")),
        "索引不完整，请缩小阅读范围或解决索引警告后创建共读"
    );
    let snapshot = open_project(&root)?;
    ensure!(
        snapshot
            .features
            .iter()
            .all(|f| f.agent_delivery.as_ref().is_none_or(|a| a.matches_source)),
        "功能交付已过期，请先更新并校验功能关联"
    );
    let head = git_command(&root)
        .args(["rev-parse", "--verify", "HEAD^{commit}"])
        .output()?;
    ensure!(head.status.success(), "共读需要已有提交的 Git 仓库");
    let commit = String::from_utf8(head.stdout)?.trim().to_owned();
    let deleted = git_command(&root)
        .args(["ls-files", "--deleted", "-z"])
        .output()?;
    ensure!(
        deleted.status.success() && deleted.stdout.is_empty(),
        "存在已删除的受控文件，请先提交本轮版本"
    );
    let mut child = git_command(&root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let result = (|| -> Result<String> {
        let mut input = child.stdin.take().context("Git 输入不可用")?;
        let mut output = BufReader::new(child.stdout.take().context("Git 输出不可用")?);
        let mut files = BTreeMap::new();
        for (path, text) in &scan.contents {
            ensure!(!path.contains(['\n', '\r', ':']), "共读路径含不支持的字符");
            writeln!(input, "HEAD:{path}")?;
            input.flush()?;
            let mut header = String::new();
            output.read_line(&mut header)?;
            let fields: Vec<_> = header.split_whitespace().collect();
            ensure!(
                fields.len() == 3 && fields[1] == "blob",
                "阅读范围内存在未提交文件：{path}"
            );
            let size = fields[2].parse::<usize>()?;
            ensure!(size <= MAX_FILE_BYTES as usize, "基线文件过大：{path}");
            let mut bytes = vec![0; size + 1];
            output.read_exact(&mut bytes)?;
            let baseline = std::str::from_utf8(&bytes[..size]).context("基线不是 UTF-8")?;
            ensure!(lf(baseline) == lf(text), "阅读范围内代码尚未提交：{path}");
            files.insert(path.clone(), digest(lf(text).as_bytes()));
        }
        Ok(digest(&serde_json::to_vec(&files)?))
    })();
    let _ = child.kill();
    let _ = child.wait();
    let source_digest = result?;
    let mut features = Vec::new();
    let mut declarations = Vec::new();
    for f in &snapshot.features {
        let modules = snapshot
            .overview
            .as_ref()
            .map(|o| {
                o.paths
                    .iter()
                    .flat_map(|p| &p.steps)
                    .filter(|s| s.feature_id == f.id)
                    .map(|s| s.module_id.clone())
                    .collect::<std::collections::BTreeSet<_>>()
                    .into_iter()
                    .collect()
            })
            .unwrap_or_default();
        let mut anchors:Vec<_>=f.anchors.iter().map(|a|json!({"id":a.id,"symbolId":a.symbol_id,"range":a.range,"role":a.role,"responsibility":a.responsibility})).collect();
        anchors.sort_by_key(|a| a["id"].as_str().unwrap_or("").to_owned());
        declarations
            .push(json!({"id":f.id,"name":f.name,"description":f.description,"anchors":anchors}));
        features.push(FeatureScope {
            id: f.id.clone(),
            name: f.name.clone(),
            modules,
            dependencies: vec![],
            evidence: f
                .anchors
                .iter()
                .map(|a| Scope {
                    path: a.range.file.clone(),
                    start_line: a.range.start_line,
                    end_line: a.range.end_line,
                })
                .collect(),
        });
    }
    let contents: BTreeMap<_, _> = scan
        .contents
        .iter()
        .map(|(p, c)| (p.clone(), c.clone()))
        .collect();
    let dependencies = crate::relations::collaboration_dependencies(
        &contents,
        &snapshot.symbols,
        &snapshot.features,
    )?;
    for feature in &mut features {
        feature.dependencies = dependencies.get(&feature.id).cloned().unwrap_or_default();
    }
    declarations.sort_by_key(|f| f["id"].as_str().unwrap_or("").to_owned());
    features.sort_by(|a, b| a.id.cmp(&b.id));
    let association_digest = digest(&serde_json::to_vec(
        &json!({"features":declarations,"overview":snapshot.overview}),
    )?);
    Ok(Manifest {
        version: Version {
            commit,
            source_digest,
            association_digest,
        },
        features,
    })
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Binding {
    server: String,
    session_id: String,
    token: String,
    invite: Option<String>,
    version: Version,
    title: String,
    pending: bool,
    #[serde(default)]
    cached: Option<Value>,
}
#[derive(Default, Serialize, Deserialize)]
struct Connections {
    active: Option<Binding>,
    history: Vec<Binding>,
}
fn file(root: &Path, private: &Path) -> Result<PathBuf> {
    fs::create_dir_all(private)?;
    ensure!(
        !fs::symlink_metadata(private)?.file_type().is_symlink(),
        "连接目录不能是符号链接"
    );
    Ok(private.join(format!(
        "{}.json",
        digest(canonical_root(root)?.to_string_lossy().as_bytes())
    )))
}
fn load(path: &Path) -> Result<Connections> {
    if !path.exists() {
        return Ok(Connections::default());
    }
    ensure!(
        !fs::symlink_metadata(path)?.file_type().is_symlink(),
        "连接文件不能是符号链接"
    );
    ensure!(fs::metadata(path)?.len() < 48 * 1024 * 1024, "连接记录超限");
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}
fn save(path: &Path, data: &Connections) -> Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(path.parent().context("连接目录不存在")?)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(&serde_json::to_vec(data)?)?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    Ok(())
}
fn field<'a>(p: &'a Value, k: &str) -> Result<&'a str> {
    p[k].as_str().with_context(|| format!("缺少 {k}"))
}
fn request(action: &str, session: &str, data: Value) -> Request {
    Request {
        action: action.into(),
        session_id: session.into(),
        data,
    }
}
fn invite(binding: &Binding) -> Result<String> {
    let mut url = reqwest_url(&binding.server)?;
    url.set_fragment(Some(&format!(
        "session={}&invite={}",
        binding.session_id,
        binding
            .invite
            .as_deref()
            .context("仅发起人持有邀请，请向发起人索取")?
    )));
    Ok(url.to_string())
}
fn reqwest_url(server: &str) -> Result<reader_collab::client::Url> {
    client::base_url(server)
}
fn validate_view(value: &Value, binding: &Binding) -> Result<()> {
    ensure!(
        value["id"].as_str() == Some(binding.session_id.as_str()),
        "服务返回了另一会话"
    );
    ensure!(
        serde_json::from_value::<Version>(value["version"].clone())? == binding.version,
        "服务返回版本不一致"
    );
    ensure!(
        value["title"].as_str().is_some()
            && value["revision"].as_u64().is_some()
            && value["closed"].as_bool().is_some()
            && value["expiresAt"].as_u64().is_some()
            && value["inviteExpiresAt"].as_u64().is_some(),
        "共读响应格式无效"
    );
    let members: Vec<reader_collab::Member> = serde_json::from_value(value["members"].clone())?;
    ensure!(
        members
            .iter()
            .any(|m| Some(m.id.as_str()) == value["me"].as_str()),
        "共读响应缺少当前成员"
    );
    ensure!(
        members.iter().all(|m| m.token_hash.is_none()),
        "共读响应含凭证字段"
    );
    serde_json::from_value::<Vec<reader_collab::Comment>>(value["comments"].clone())?;
    serde_json::from_value::<Vec<reader_collab::Task>>(value["tasks"].clone())?;
    serde_json::from_value::<Vec<reader_collab::FeatureScope>>(value["features"].clone())?;
    serde_json::from_value::<Vec<reader_collab::Hint>>(value["hints"].clone())?;
    Ok(())
}

/// Only Tauri's currently opened root may call this. No credentials enter project .reader/.
pub fn action(root: &Path, private: &Path, action: &str, payload: Value) -> Result<Value> {
    let path = file(root, private)?;
    let mut state = load(&path)?;
    if action == "history" {
        return Ok(json!(state
            .history
            .iter()
            .map(|b| json!({"id":b.session_id,"title":b.title,"version":b.version}))
            .collect::<Vec<_>>()));
    }
    if action == "leave" {
        if let Some(b) = state.active.take() {
            state.history.retain(|h| h.session_id != b.session_id);
            state.history.push(b);
            if state.history.len() > 8 {
                state.history.remove(0);
            }
        }
        save(&path, &state)?;
        return Ok(Value::Null);
    }
    if action == "resume" {
        let id = field(&payload, "id")?;
        let b = state
            .history
            .iter()
            .find(|b| b.session_id == id)
            .context("历史会话不存在")?
            .clone();
        state.active = Some(b);
        save(&path, &state)?;
    }
    if action == "manifest" {
        return Ok(serde_json::to_value(manifest(root)?)?);
    }
    if action == "create" || action == "join" {
        let m = manifest(root)?;
        let (server, id, inv) = if action == "join" {
            let mut url = client::Url::parse(field(&payload, "invitation")?)?;
            let pairs: std::collections::BTreeMap<_, _> = client::form_urlencoded::parse(
                url.fragment().context("邀请缺少会话信息")?.as_bytes(),
            )
            .into_owned()
            .collect();
            let id = pairs.get("session").context("邀请缺少会话")?.clone();
            let inv = pairs.get("invite").context("邀请缺少口令")?.clone();
            url.set_fragment(None);
            (url.to_string(), id, Some(inv))
        } else {
            (
                field(&payload, "server")?.to_owned(),
                Uuid::new_v4().to_string(),
                Some(reader_collab::secret()),
            )
        };
        client::endpoint(&server)?;
        let reuse = state
            .active
            .as_ref()
            .filter(|b| {
                b.pending
                    && b.server == server
                    && (action == "create" || b.session_id == id)
                    && b.version == m.version
            })
            .cloned();
        let b = reuse.unwrap_or(Binding {
            server,
            session_id: id,
            token: reader_collab::secret(),
            invite: if action == "create" {
                inv.clone()
            } else {
                None
            },
            version: m.version.clone(),
            title: payload["title"].as_str().unwrap_or("共读").into(),
            pending: true,
            cached: None,
        });
        ensure!(
            state.active.as_ref().is_none_or(|a| a.pending),
            "请先离开当前会话，再创建或加入另一轮"
        );
        state.active = Some(b.clone());
        save(&path, &state)?;
        let data = if action == "create" {
            json!({"id":b.session_id,"ownerToken":b.token,"inviteToken":b.invite,"manifest":m,"name":field(&payload,"name")?,"title":field(&payload,"title")?})
        } else {
            json!({"version":m.version,"memberToken":b.token,"inviteToken":inv,"name":field(&payload,"name")?})
        };
        let value = client::call(
            &b.server,
            if action == "create" {
                field(&payload, "createKey")?
            } else {
                ""
            },
            &request(action, &b.session_id, data),
        )?;
        validate_view(&value, &b)?;
        let active = state.active.as_mut().unwrap();
        active.pending = false;
        active.title = value["title"].as_str().unwrap_or("共读").into();
        active.cached = Some(value.clone());
        save(&path, &state)?;
        return Ok(value);
    }
    let Some(b) = state.active.as_ref() else {
        return Ok(Value::Null);
    };
    if action == "addresses" {
        return Ok(json!(local_addresses()));
    }
    if action == "advertise" {
        let b = state.active.as_mut().unwrap();
        let cached = b.cached.as_ref().context("尚未同步讨论")?;
        ensure!(
            cached["members"].as_array().is_some_and(|members| members
                .iter()
                .any(|m| m["id"] == cached["me"] && m["role"] == "host")),
            "仅发起人可调整本机邀请地址"
        );
        let ip = field(&payload, "address")?;
        ensure!(
            local_addresses().iter().any(|local| local == ip),
            "请选择本机可用的局域网地址"
        );
        let mut url = client::base_url(&b.server)?;
        ensure!(url.set_host(Some(ip)).is_ok(), "地址无效");
        b.server = url.to_string();
        let value = b.cached.clone().unwrap();
        save(&path, &state)?;
        return Ok(value);
    }
    if action == "invitation" {
        return Ok(Value::String(invite(b)?));
    }
    if action == "location" {
        ensure!(
            manifest(root)?.version == b.version,
            "代码或功能关联已变化，不能将旧讨论定位到新代码；请回到本轮版本"
        );
        let scope: Scope = if payload["scope"].is_object() {
            serde_json::from_value(payload["scope"].clone())?
        } else {
            let project = open_project(root)?;
            let f = project
                .features
                .iter()
                .find(|f| Some(f.id.as_str()) == payload["featureId"].as_str())
                .context("功能不存在")?;
            let a = f.anchors.first().context("此功能没有代码证据")?;
            Scope {
                path: a.range.file.clone(),
                start_line: a.range.start_line,
                end_line: a.range.end_line,
            }
        };
        ensure!(reader_collab::tasks::scope_valid(&scope), "批注位置无效");
        let source = read_file(root, &scope.path)?;
        ensure!(
            scope.end_line as usize <= source.content.lines().count().max(1),
            "批注位置超出当前文件"
        );
        return Ok(
            json!({"path":scope.path,"startLine":scope.start_line,"endLine":scope.end_line,"fileHash":source.hash}),
        );
    }
    if action == "export" {
        let cached = b.cached.as_ref().context("尚未同步讨论")?;
        let tasks = serde_json::from_value::<Vec<reader_collab::Task>>(cached["tasks"].clone())?;
        let comments =
            serde_json::from_value::<Vec<reader_collab::Comment>>(cached["comments"].clone())?;
        let members =
            serde_json::from_value::<Vec<reader_collab::Member>>(cached["members"].clone())?;
        let text = reader_collab::tasks::export(
            &tasks,
            &comments,
            &members,
            &b.version,
            &serde_json::from_value::<Vec<FeatureScope>>(cached["features"].clone())?,
            field(&payload, "ownerId")?,
        )?;
        return Ok(json!(format!(
            "本机最近同步的讨论版本：{}。导出不代表任务已完成或已复核。\n\n{}",
            cached["revision"], text
        )));
    }
    let remote = if action == "read" || action == "resume" {
        "read"
    } else {
        action
    };
    let mut data = payload;
    if ["comment", "task", "mergeTasks"].contains(&remote) {
        ensure!(
            manifest(root)?.version == b.version,
            "本地代码或功能关联已变化，请回到本轮版本；新版本应另建一轮共读"
        );
    }
    if remote == "taskStatus" && data["status"].as_str() == Some("reviewed") {
        let reviewed = manifest(root)?;
        ensure!(
            Some(reviewed.version.commit.as_str()) == data["resultCommit"].as_str(),
            "请先用 Git 切换到负责人交付的提交，刷新并实际阅读后再确认复核"
        );
    }
    let new_invite = if remote == "rotateInvite" {
        let token = reader_collab::secret();
        data["inviteToken"] = json!(token);
        Some(token)
    } else {
        None
    };
    if remote == "comment" {
        if let Some(scope) = data["comment"]["scope"].as_object() {
            let path = scope
                .get("path")
                .and_then(Value::as_str)
                .context("批注路径无效")?;
            let source = read_file(root, path)?;
            if let Some(hash) = data["localFileHash"].as_str() {
                ensure!(
                    hash == source.hash,
                    "引用的个人批注对应旧代码，请重新核对后再共享"
                );
            }
            ensure!(
                data["comment"]["scope"]["endLine"]
                    .as_u64()
                    .is_some_and(|line| line <= source.content.lines().count().max(1) as u64),
                "批注位置不在当前代码中"
            );
        }
    }
    let result = match client::call(&b.server, &b.token, &request(remote, &b.session_id, data)) {
        Ok(value) => value,
        Err(error) if remote == "read" => {
            if let Some(mut cached) = b.cached.clone() {
                cached["offline"] = json!(true);
                cached["connectionError"] = json!(error.to_string());
                return Ok(cached);
            }
            return Err(error);
        }
        Err(error) => return Err(error),
    };
    validate_view(&result, b)?;
    let update = state
        .active
        .as_ref()
        .and_then(|b| b.cached.as_ref())
        .and_then(|c| c["revision"].as_u64())
        != result["revision"].as_u64();
    if update {
        state.active.as_mut().unwrap().cached = Some(result.clone());
        save(&path, &state)?;
    }
    if let Some(token) = new_invite {
        state.active.as_mut().unwrap().invite = Some(token);
        save(&path, &state)?;
    }
    Ok(result)
}
