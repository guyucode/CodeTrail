use crate::*;
use anyhow::{bail, ensure, Context, Result};
use fs2::FileExt;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
};
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Room {
    id: String,
    title: String,
    manifest: Manifest,
    revision: u64,
    expires_at: u64,
    invite_hash: String,
    invite_expires: u64,
    closed: bool,
    members: Vec<Member>,
    comments: Vec<Comment>,
    tasks: Vec<Task>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Database {
    rooms: BTreeMap<String, Room>,
}
pub struct Service {
    data: Mutex<Database>,
    path: PathBuf,
    admin_hash: String,
    _lock: File,
}
fn text(data: &Value, key: &str, max: usize) -> Result<String> {
    let v = data[key].as_str().context(format!("缺少 {key}"))?;
    ensure!(
        !v.trim().is_empty() && v.len() <= max && !v.contains('\0'),
        "{key} 长度或内容无效"
    );
    Ok(v.to_owned())
}
fn identifier(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}
fn hash_equal(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |v, (a, b)| v | (a ^ b)) == 0
}
fn check_manifest(m: &Manifest) -> Result<()> {
    for hash in [&m.version.source_digest, &m.version.association_digest] {
        ensure!(
            hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
            "版本摘要无效"
        );
    }
    ensure!(
        [40, 64].contains(&m.version.commit.len())
            && m.version.commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "提交 ID 无效"
    );
    ensure!(m.features.len() <= 500, "功能数量超限");
    let mut ids = std::collections::BTreeSet::new();
    for f in &m.features {
        ensure!(
            identifier(&f.id)
                && ids.insert(&f.id)
                && !f.name.trim().is_empty()
                && f.name.len() <= 500
                && f.evidence.len() <= 200
                && f.evidence.iter().all(tasks::scope_valid)
                && f.modules.len() <= 100
                && f.modules.iter().all(|m| identifier(m)),
            "功能范围无效"
        );
    }
    Ok(())
}
fn view(room: &Room, me: &Member) -> Value {
    let active = me.status == "active";
    let mut members = if active {
        room.members.clone()
    } else {
        vec![me.clone()]
    };
    for m in &mut members {
        m.token_hash = None;
    }
    json!({"id":room.id,"title":room.title,"version":room.manifest.version,"revision":room.revision,"closed":room.closed,"expiresAt":room.expires_at,"inviteExpiresAt":room.invite_expires,"me":me.id,"members":members,"features":if active{room.manifest.features.clone()}else{vec![]},"comments":if active{room.comments.clone()}else{vec![]},"tasks":if active{room.tasks.clone()}else{vec![]},"hints":if active{tasks::hints(&room.tasks,&room.manifest.features)}else{vec![]}})
}
impl Service {
    pub fn open(directory: &Path, admin: &str) -> Result<Self> {
        ensure!(admin.len() >= 32, "服务创建密钥至少 32 字符");
        fs::create_dir_all(directory)?;
        let path = directory.join("collaboration.json");
        let lock_path = directory.join("service.lock");
        for p in [&path, &lock_path] {
            ensure!(
                !fs::symlink_metadata(p).is_ok_and(|m| m.file_type().is_symlink()),
                "服务数据不能是符号链接"
            );
        }
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(lock_path)?;
        lock.try_lock_exclusive()
            .context("数据目录已被另一个服务占用")?;
        let data = if path.exists() {
            ensure!(
                fs::metadata(&path)?.len() < 128 * 1024 * 1024,
                "服务数据超限"
            );
            serde_json::from_slice(&fs::read(&path)?)?
        } else {
            Database::default()
        };
        Ok(Self {
            data: Mutex::new(data),
            path,
            admin_hash: digest(admin.as_bytes()),
            _lock: lock,
        })
    }
    fn persist(&self, data: &Database) -> Result<()> {
        let bytes = serde_json::to_vec(data)?;
        ensure!(
            bytes.len() < 128 * 1024 * 1024,
            "服务存储已满，请管理员归档"
        );
        let mut tmp = tempfile::NamedTempFile::new_in(self.path.parent().unwrap())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.as_file()
                .set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&self.path)?;
        Ok(())
    }
    pub fn handle(&self, token: &str, request: Request) -> Result<Value> {
        let mut locked = self
            .data
            .lock()
            .map_err(|_| anyhow::anyhow!("服务暂不可用"))?;
        let mut db = locked.clone();
        let d = &request.data;
        if request.action == "create" {
            ensure!(
                hash_equal(&digest(token.as_bytes()), &self.admin_hash),
                "无权创建会话"
            );
            let id = text(d, "id", 128)?;
            ensure!(identifier(&id), "会话 ID 无效");
            let owner_token = text(d, "ownerToken", 128)?;
            let invite = text(d, "inviteToken", 128)?;
            ensure!(owner_token.len() >= 32 && invite.len() >= 32, "凭证过短");
            if let Some(room) = db.rooms.get(&id) {
                let me = &room.members[0];
                ensure!(
                    hash_equal(
                        me.token_hash.as_deref().unwrap_or(""),
                        &digest(owner_token.as_bytes())
                    ),
                    "会话 ID 已存在"
                );
                return Ok(view(room, me));
            }
            ensure!(db.rooms.len() < 50, "会话数量已达上限，请管理员归档");
            let manifest: Manifest = serde_json::from_value(d["manifest"].clone())?;
            check_manifest(&manifest)?;
            let owner = Member {
                id: uuid::Uuid::new_v4().to_string(),
                name: text(d, "name", 80)?,
                role: "host".into(),
                status: "active".into(),
                token_hash: Some(digest(owner_token.as_bytes())),
            };
            let room = Room {
                id: id.clone(),
                title: text(d, "title", 120)?,
                manifest,
                revision: 1,
                expires_at: now() + 30 * 86400,
                invite_hash: digest(invite.as_bytes()),
                invite_expires: now() + 86400,
                closed: false,
                members: vec![owner],
                comments: vec![],
                tasks: vec![],
            };
            let result = view(&room, &room.members[0]);
            db.rooms.insert(id, room);
            self.persist(&db)?;
            *locked = db;
            return Ok(result);
        }
        let room = db
            .rooms
            .get_mut(&request.session_id)
            .context("会话不存在")?;
        ensure!(now() < room.expires_at, "会话已过期，请导出后重新建立");
        if request.action == "join" {
            let version: Version = serde_json::from_value(d["version"].clone())?;
            ensure!(
                version == room.manifest.version,
                "代码或功能关联版本不同，无法加入本轮共读"
            );
            let member_token = text(d, "memberToken", 128)?;
            ensure!(member_token.len() >= 32, "凭证过短");
            let hash = digest(member_token.as_bytes());
            if let Some(me) = room
                .members
                .iter()
                .find(|m| hash_equal(m.token_hash.as_deref().unwrap_or(""), &hash))
            {
                ensure!(me.status != "removed", "已被移出会话");
                return Ok(view(room, me));
            }
            ensure!(
                !room.closed
                    && now() < room.invite_expires
                    && hash_equal(
                        &digest(text(d, "inviteToken", 128)?.as_bytes()),
                        &room.invite_hash
                    ),
                "邀请无效、已撤销或已过期"
            );
            ensure!(
                room.members
                    .iter()
                    .filter(|m| m.status != "removed")
                    .count()
                    < 8
                    && room.members.len() < 64,
                "会话成员已满"
            );
            let me = Member {
                id: uuid::Uuid::new_v4().to_string(),
                name: text(d, "name", 80)?,
                role: "member".into(),
                status: "pending".into(),
                token_hash: Some(hash),
            };
            let result = view(room, &me);
            room.members.push(me);
            room.revision += 1;
            self.persist(&db)?;
            *locked = db;
            return Ok(result);
        }
        let index = room
            .members
            .iter()
            .position(|m| {
                hash_equal(
                    m.token_hash.as_deref().unwrap_or(""),
                    &digest(token.as_bytes()),
                )
            })
            .context("成员凭证无效")?;
        let me = room.members[index].clone();
        ensure!(me.status != "removed", "已被移出会话");
        if request.action == "read" {
            return Ok(view(room, &me));
        }
        ensure!(me.status == "active", "等待发起人批准加入");
        if request.action == "export" {
            let owner = text(d, "ownerId", 128)?;
            return Ok(Value::String(tasks::export(
                &room.tasks,
                &room.comments,
                &room.members,
                &room.manifest.version,
                &room.manifest.features,
                &owner,
            )?));
        }
        ensure!(!room.closed, "本轮讨论已结束，可导出后建立新一轮");
        if request.action == "comment" {
            let input: CommentInput = serde_json::from_value(d["comment"].clone())?;
            if let Some(c) = room.comments.iter().find(|c| c.input.id == input.id) {
                ensure!(c.author_id == me.id && c.input == input, "批注 ID 冲突");
                return Ok(view(room, &me));
            }
        }
        ensure!(
            d["expectedRevision"].as_u64() == Some(room.revision),
            "讨论已被他人更新，请刷新后重试；草稿未提交"
        );
        let host = me.role == "host";
        match request.action.as_str() {
            "comment" => {
                let input: CommentInput = serde_json::from_value(d["comment"].clone())?;
                ensure!(
                    identifier(&input.id)
                        && !input.body.trim().is_empty()
                        && input.body.len() <= 8000
                        && room.comments.len() < 500,
                    "批注为空或超过本轮上限"
                );
                if let Some(id) = &input.feature_id {
                    ensure!(
                        room.manifest.features.iter().any(|f| &f.id == id),
                        "批注功能不存在"
                    );
                }
                if let Some(s) = &input.scope {
                    ensure!(tasks::scope_valid(s), "批注位置无效");
                }
                if let Some(id) = &input.parent_id {
                    ensure!(
                        room.comments.iter().any(|c| &c.input.id == id),
                        "原批注不存在"
                    );
                }
                room.comments.push(Comment {
                    input,
                    author_id: me.id.clone(),
                    created_at: now(),
                });
            }
            "member" => {
                ensure!(host, "只有发起人可管理成员");
                let id = text(d, "memberId", 128)?;
                let status = text(d, "status", 16)?;
                ensure!(
                    ["active", "removed"].contains(&status.as_str()),
                    "成员状态无效"
                );
                let member = room
                    .members
                    .iter_mut()
                    .find(|m| m.id == id)
                    .context("成员不存在")?;
                ensure!(member.role != "host", "不能移除发起人");
                member.status = status;
            }
            "rotateInvite" => {
                ensure!(host, "只有发起人可更新邀请");
                let invite = text(d, "inviteToken", 128)?;
                ensure!(invite.len() >= 32, "凭证过短");
                room.invite_hash = digest(invite.as_bytes());
                room.invite_expires = now() + 86400;
            }
            "close" => {
                ensure!(host, "只有发起人可结束本轮");
                room.closed = true;
            }
            "mergeTasks" => {
                ensure!(host, "只有发起人可合并任务");
                let target = text(d, "targetId", 128)?;
                let sources: Vec<String> = serde_json::from_value(d["sourceIds"].clone())?;
                ensure!(
                    !sources.is_empty() && sources.len() <= 30 && !sources.contains(&target),
                    "请选择要并入的其他任务"
                );
                ensure!(
                    room.tasks
                        .iter()
                        .any(|t| t.input.id == target && t.status == "pending"),
                    "目标须为待处理任务"
                );
                ensure!(
                    sources.iter().all(|id| room
                        .tasks
                        .iter()
                        .any(|t| &t.input.id == id && t.status == "pending")),
                    "只能合并待处理任务，请先协调已接手的工作"
                );
                let originals: Vec<_> = room
                    .tasks
                    .iter()
                    .filter(|t| sources.contains(&t.input.id))
                    .cloned()
                    .collect();
                let into = room
                    .tasks
                    .iter_mut()
                    .find(|t| t.input.id == target)
                    .unwrap();
                for source in &originals {
                    into.input.goal +=
                        &format!("\n合并「{}」：{}", source.input.title, source.input.goal);
                    into.input.acceptance += &format!("\n{}", source.input.acceptance);
                    into.input.boundary += &format!("\n{}", source.input.boundary);
                    for id in &source.input.comment_ids {
                        if !into.input.comment_ids.contains(id) {
                            into.input.comment_ids.push(id.clone());
                        }
                    }
                    for id in &source.input.feature_ids {
                        if !into.input.feature_ids.contains(id) {
                            into.input.feature_ids.push(id.clone());
                        }
                    }
                    for scope in &source.input.scopes {
                        if !into.input.scopes.contains(scope) {
                            into.input.scopes.push(scope.clone());
                        }
                    }
                    for contract in &source.input.contracts {
                        if !into.input.contracts.contains(contract) {
                            into.input.contracts.push(contract.clone());
                        }
                    }
                    for dep in &source.input.depends_on {
                        if !into.input.depends_on.contains(dep) {
                            into.input.depends_on.push(dep.clone());
                        }
                    }
                }
                ensure!(
                    into.input.goal.len() <= 8000
                        && into.input.acceptance.len() <= 4000
                        && into.input.boundary.len() <= 4000
                        && into.input.comment_ids.len() <= 100,
                    "合并内容过大，请先精简任务"
                );
                into.input
                    .depends_on
                    .retain(|id| id != &target && !sources.contains(id));
                into.revision = room.revision + 1;
                for t in &mut room.tasks {
                    if sources.contains(&t.input.id) {
                        t.status = "merged".into();
                        t.merged_into = Some(target.clone());
                        t.revision = room.revision + 1;
                    } else if t.input.id != target {
                        if t.input.depends_on.iter().any(|id| sources.contains(id)) {
                            ensure!(
                                t.status == "pending" || t.status == "merged",
                                "下游任务已接手，先退回协调再合并前置任务"
                            );
                        }
                        for dep in &mut t.input.depends_on {
                            if sources.contains(dep) {
                                *dep = target.clone();
                            }
                        }
                        t.input.depends_on.sort();
                        t.input.depends_on.dedup();
                    }
                }
                tasks::validate_assignments(&room.tasks, &room.manifest.features)?;
            }
            "task" => {
                ensure!(host, "第一版由发起人整合和分配任务");
                let mut input: TaskInput = serde_json::from_value(d["task"].clone())?;
                ensure!(
                    identifier(&input.id)
                        && !input.title.trim().is_empty()
                        && input.title.len() <= 300
                        && !input.goal.trim().is_empty()
                        && input.goal.len() <= 8000
                        && !input.acceptance.trim().is_empty()
                        && input.acceptance.len() <= 4000
                        && !input.boundary.trim().is_empty()
                        && input.boundary.len() <= 4000
                        && input.coupling_note.len() <= 4000,
                    "任务须填写目标、验收和修改边界"
                );
                ensure!(
                    !input.comment_ids.is_empty()
                        && input.comment_ids.len() <= 100
                        && input.feature_ids.len() <= 100
                        && input.scopes.len() <= 200
                        && input.contracts.len() <= 50
                        && input.depends_on.len() <= 100,
                    "任务范围为空或超限"
                );
                ensure!(
                    input
                        .comment_ids
                        .iter()
                        .all(|id| room.comments.iter().any(|c| &c.input.id == id)),
                    "任务来源批注不存在"
                );
                ensure!(
                    input.feature_ids.iter().all(|id| room
                        .manifest
                        .features
                        .iter()
                        .any(|f| &f.id == id))
                        && input.scopes.iter().all(tasks::scope_valid)
                        && input
                            .contracts
                            .iter()
                            .all(|s| !s.trim().is_empty() && s.len() <= 200),
                    "任务关联范围无效"
                );
                if let Some(owner) = &input.owner_id {
                    ensure!(
                        room.members
                            .iter()
                            .any(|m| &m.id == owner && m.status == "active"),
                        "负责人必须是已批准成员"
                    );
                }
                for c in room
                    .comments
                    .iter()
                    .filter(|c| input.comment_ids.contains(&c.input.id))
                {
                    if let Some(id) = &c.input.feature_id {
                        if !input.feature_ids.contains(id) {
                            input.feature_ids.push(id.clone());
                        }
                    }
                    if let Some(scope) = &c.input.scope {
                        if !input.scopes.contains(scope) {
                            input.scopes.push(scope.clone());
                        }
                    }
                }
                ensure!(
                    room.tasks
                        .iter()
                        .filter(|t| t.input.depends_on.contains(&input.id))
                        .all(|t| t.status == "pending"),
                    "修改前置任务前，请先将下游任务退回待处理并协调"
                );
                let task = Task {
                    input,
                    status: "pending".into(),
                    revision: room.revision + 1,
                    result_commit: None,
                    review_note: None,
                    merged_into: None,
                };
                if let Some(t) = room.tasks.iter_mut().find(|t| t.input.id == task.input.id) {
                    ensure!(t.status != "merged", "已合并任务只保留历史，请修改目标任务");
                    *t = task;
                } else {
                    ensure!(room.tasks.len() < 60, "任务数量超限");
                    room.tasks.push(task);
                }
                tasks::validate_assignments(&room.tasks, &room.manifest.features)?;
            }
            "taskStatus" => {
                let id = text(d, "taskId", 128)?;
                let status = text(d, "status", 32)?;
                let t = room
                    .tasks
                    .iter()
                    .find(|t| t.input.id == id)
                    .context("任务不存在")?;
                ensure!(t.status != "merged", "已合并任务不能重新接手");
                ensure!(
                    host || t.input.owner_id.as_deref() == Some(me.id.as_str()),
                    "只有负责人或发起人可更新任务"
                );
                ensure!(
                    ["pending", "accepted", "needsReview", "reviewed"].contains(&status.as_str()),
                    "任务状态无效"
                );
                if status == "reviewed" {
                    ensure!(host, "仅发起人能在人工复核后确认完成");
                    ensure!(t.status == "needsReview", "先提交待复核结果");
                    text(d, "reviewNote", 4000)?;
                    text(d, "resultCommit", 64)?;
                }
                if ["accepted", "needsReview"].contains(&status.as_str()) {
                    for dep in &t.input.depends_on {
                        ensure!(
                            room.tasks
                                .iter()
                                .any(|t| &t.input.id == dep && t.status == "reviewed"),
                            "前置任务尚未复核，请先协调顺序"
                        );
                    }
                }
                if ["accepted", "needsReview"].contains(&status.as_str()) {
                    ensure!(
                        t.input.owner_id.as_deref() == Some(me.id.as_str()),
                        "须由任务负责人本人确认接手或提交交付"
                    );
                }
                if status == "reviewed" {
                    ensure!(
                        d["resultCommit"].as_str() == t.result_commit.as_deref(),
                        "人工复核必须对应负责人实际提交的版本"
                    );
                }
                if status == "accepted" {
                    ensure!(t.status == "pending", "请从待处理状态接手任务");
                }
                if status == "needsReview" {
                    ensure!(t.status == "accepted", "请先确认接手任务");
                }
                if status == "pending" {
                    ensure!(
                        room.tasks
                            .iter()
                            .filter(|t| t.input.depends_on.contains(&id))
                            .all(|t| t.status == "pending"),
                        "请先退回并协调下游任务"
                    );
                }
                let result_commit = if ["needsReview", "reviewed"].contains(&status.as_str()) {
                    let c = text(d, "resultCommit", 64)?;
                    ensure!(
                        [40, 64].contains(&c.len()) && c.bytes().all(|b| b.is_ascii_hexdigit()),
                        "请填写实际交付提交 ID"
                    );
                    Some(c)
                } else {
                    None
                };
                let t = room.tasks.iter_mut().find(|t| t.input.id == id).unwrap();
                t.review_note = if status == "reviewed" {
                    Some(text(d, "reviewNote", 4000)?)
                } else {
                    None
                };
                t.result_commit = result_commit;
                t.status = status;
                t.revision = room.revision + 1;
            }
            _ => bail!("未知共读操作"),
        }
        room.revision += 1;
        ensure!(
            serde_json::to_vec(room)?.len() <= 4 * 1024 * 1024,
            "本轮讨论达到容量限制，请导出并另建会话"
        );
        let result = view(room, &me);
        self.persist(&db)?;
        *locked = db;
        Ok(result)
    }
}
