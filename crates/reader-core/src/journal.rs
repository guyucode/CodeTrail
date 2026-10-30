//! Separate Agent activity and human annotation journals. Never alter review state.
use crate::*;
use std::time::Duration;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Activity {
    // Optional only to deserialize legacy records; new reports must supply all three.
    #[serde(default)]
    pub project_root: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub files: Vec<String>,
    pub id: String,
    pub task: String,
    pub stage: String,
    pub places: Vec<String>,
    pub impact: String,
    pub expected_effect: String,
    pub verification: Option<String>,
    #[serde(default)]
    pub recorded_at: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Annotation {
    pub id: String,
    pub body: String,
    pub context: String,
    pub path: Option<String>,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
    pub file_hash: Option<String>,
    pub feature_id: Option<String>,
    pub feature_name: Option<String>,
    pub quote: String,
    pub version: String,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Journal {
    pub activities: Vec<Activity>,
    pub annotations: Vec<Annotation>,
}

struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn lock(root: &Path) -> Result<Lock> {
    let dir = metadata_root(root)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("journal.lock");
    reject_symlink(&path)?;
    // Small bounded wait allows independent Agent reports and human autosaves.
    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(Lock(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
    bail!("记录正在保存；请稍后重试。异常退出后需人工检查 journal.lock")
}
fn load<T: serde::de::DeserializeOwned + Default>(root: &Path, name: &str) -> Result<T> {
    let path = metadata_root(root)?.join(name);
    reject_symlink(&path)?;
    if !path.exists() {
        return Ok(T::default());
    }
    serde_json::from_str(&read_limited_text(&path, MAX_METADATA_BYTES)?)
        .context("记录损坏，保留原文件，请先备份修复")
}
fn id(value: &str) -> Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "记录 ID 无效"
    );
    Ok(())
}
fn text(value: &str, max: usize) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.chars().count() <= max,
        "记录文字为空或过长"
    );
    Ok(())
}
fn activity_path(path: &str) -> Result<String> {
    let path = normalize_relative(path)?;
    ensure!(
        ![".reader", ".git"].contains(&path.split('/').next().unwrap_or("")),
        "阅读器元数据和 Git 内部文件不是项目修改动态"
    );
    ensure!(
        !path.contains(['*', '?']),
        "动态须列出具体文件路径，不能使用通配符"
    );
    Ok(path)
}
/// Scope is explicit, never inferred from a task title or the storage directory.
fn belongs_to_project(root: &Path, event: &Activity) -> bool {
    event.kind.as_deref() == Some("projectChange")
        && event.project_root.as_deref() == root.to_str()
        && !event.files.is_empty()
        && event.files.len() <= 30
        && event.files.iter().all(|path| activity_path(path).is_ok())
}
pub fn read(root: &Path) -> Result<Journal> {
    let root = canonical_root(root)?;
    Ok(Journal {
        activities: load::<Vec<Activity>>(&root, "activity.json")?
            .into_iter()
            .filter(|event| belongs_to_project(&root, event))
            .collect(),
        annotations: load(&root, "annotations.json")?,
    })
}
pub fn report(root: &Path, mut event: Activity) -> Result<Activity> {
    let root = canonical_root(root)?;
    ensure!(belongs_to_project(&root, &event), "动态必须声明 kind=projectChange、与目标根目录完全一致的 projectRoot，以及 1–30 个项目内的 files；运行结果请写入 trace");
    let mut unique_files = HashSet::new();
    for path in &mut event.files {
        *path = activity_path(path)?;
        ensure!(unique_files.insert(path.clone()), "动态文件路径重复");
        let mut full = root.clone();
        for component in Path::new(path).components() {
            full.push(component);
            reject_symlink(&full)?;
        }
        ensure!(!full.is_dir(), "动态应列出具体文件，而非目录");
    }
    event.project_root = Some(root.to_string_lossy().into_owned());
    id(&event.id)?;
    text(&event.task, 200)?;
    text(&event.impact, 4000)?;
    text(&event.expected_effect, 4000)?;
    ensure!(
        ["working", "checking", "delivered", "blocked"].contains(&event.stage.as_str()),
        "未知工作阶段"
    );
    ensure!(
        !event.places.is_empty() && event.places.len() <= 30,
        "改动位置须为 1–30 项"
    );
    for place in &event.places {
        text(place, 500)?;
    }
    if let Some(v) = &event.verification {
        text(v, 4000)?;
    }
    let _lock = lock(&root)?;
    let mut events: Vec<Activity> = load(&root, "activity.json")?;
    ensure!(
        !events.iter().any(|e| e.id == event.id),
        "动态 ID 已存在；请为新事件使用新 ID"
    );
    event.recorded_at = now();
    events.push(event.clone());
    if events.len() > 200 {
        events.drain(..events.len() - 200);
    }
    atomic_json(&metadata_root(&root)?.join("activity.json"), &events)?;
    Ok(event)
}
fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
pub fn save_annotation(
    root: &Path,
    mut note: Annotation,
    expected_version: &str,
) -> Result<Annotation> {
    let root = canonical_root(root)?;
    id(&note.id)?;
    text(&note.body, 20000)?;
    text(&note.context, 2000)?;
    ensure!(note.quote.chars().count() <= 4000, "引用过长");
    for value in [&note.feature_id, &note.feature_name, &note.file_hash]
        .into_iter()
        .flatten()
    {
        text(value, 500)?;
    }
    if let Some(path) = &note.path {
        normalize_relative(path)?;
    }
    if let Some(start) = note.start_line {
        ensure!(
            start > 0 && note.end_line.is_some_and(|end| end >= start),
            "批注行范围无效"
        );
    }
    ensure!(
        note.start_line.is_some() == note.end_line.is_some(),
        "批注行范围不完整"
    );
    let _lock = lock(&root)?;
    let mut notes: Vec<Annotation> = load(&root, "annotations.json")?;
    let existing = notes.iter().position(|n| n.id == note.id);
    ensure!(
        existing.map(|i| notes[i].version.as_str()).unwrap_or("") == expected_version,
        "批注已在别处修改，请保留草稿并重新读取"
    );
    note.version = new_id();
    if let Some(i) = existing {
        notes[i] = note.clone();
    } else {
        ensure!(notes.len() < 1000, "批注已达 1000 条上限");
        notes.push(note.clone());
    }
    atomic_json(&metadata_root(&root)?.join("annotations.json"), &notes)?;
    Ok(note)
}
pub fn delete_annotation(root: &Path, id: &str, expected_version: &str) -> Result<()> {
    let root = canonical_root(root)?;
    let _lock = lock(&root)?;
    let mut notes: Vec<Annotation> = load(&root, "annotations.json")?;
    let index = notes
        .iter()
        .position(|n| n.id == id)
        .context("批注不存在")?;
    ensure!(
        notes[index].version == expected_version,
        "批注已变化，请重新读取"
    );
    notes.remove(index);
    atomic_json(&metadata_root(&root)?.join("annotations.json"), &notes)
}
pub fn export_markdown(root: &Path) -> Result<String> {
    let root = canonical_root(root)?;
    let notes: Vec<Annotation> = load(&root, "annotations.json")?;
    let mut out = format!("# 阅读批注\n\n项目：{}\n\n以下为人工反馈，请先核对当前源码与记录版本，再修改；不得自动确认阅读或清除批注。\n", markdown_text(&root.display().to_string()));
    for (i, n) in notes.iter().enumerate() {
        out.push_str(&format!("\n## {} · {}\n\n- 批注 ID：{}\n- 上下文：{}\n- 功能：{} ({})\n- 位置：{} L{}–{}\n- 记录时文件 SHA-256：{}\n\n### 我的批注\n\n{}\n", i+1, markdown_text(&n.context), n.id, markdown_text(&n.context), markdown_text(n.feature_name.as_deref().unwrap_or("项目")), markdown_text(n.feature_id.as_deref().unwrap_or("无")), markdown_text(n.path.as_deref().unwrap_or("项目概览")), n.start_line.unwrap_or(0), n.end_line.unwrap_or(0), n.file_hash.as_deref().unwrap_or("无"), n.body));
        if !n.quote.is_empty() {
            out.push_str("\n### 当时的引用\n\n");
            for line in n.quote.lines() {
                out.push_str(&format!("> {}\n", line));
            }
        }
    }
    Ok(out)
}
/// Export only to a fixed human-owned metadata file; never accept source paths.
pub fn export_file(root: &Path) -> Result<String> {
    let root = canonical_root(root)?;
    let _lock = lock(&root)?;
    let content = export_markdown(&root)?;
    let path = metadata_root(&root)?.join("annotations.md");
    reject_symlink(&path)?;
    let temp = path.with_file_name(format!(".{}.md.tmp", new_id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        reject_symlink(&path)?;
        fs::rename(&temp, &path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result?;
    Ok(path.display().to_string())
}
