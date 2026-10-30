//! Bounded runtime observations. Producers explicitly select values; the reader never executes them.
use crate::*;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunStart {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub producer: String,
    pub summary: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLocation {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub file_hash: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ObservedValue {
    pub name: String,
    pub value: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeStep {
    pub id: String,
    pub title: String,
    pub status: String,
    pub result: String,
    pub reason: String,
    pub inputs: Vec<ObservedValue>,
    pub outputs: Vec<ObservedValue>,
    pub location: RuntimeLocation,
    pub elapsed_ms: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepRequest {
    pub run_id: String,
    pub step: RuntimeStep,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunFinish {
    pub run_id: String,
    pub status: String,
    pub summary: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordedStep {
    pub sequence: usize,
    pub recorded_at: String,
    pub observation: RuntimeStep,
    pub source_excerpt: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeRun {
    pub info: RunStart,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub revision: usize,
    pub steps: Vec<RecordedStep>,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    #[serde(default = "schema_version")]
    schema_version: u32,
    runs: Vec<RuntimeRun>,
}
fn schema_version() -> u32 {
    1
}
impl Default for Store {
    fn default() -> Self {
        Self {
            schema_version: 1,
            runs: vec![],
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunSummary {
    pub info: RunStart,
    pub status: String,
    pub started_at: String,
    pub finished_at: Option<String>,
    pub revision: usize,
    pub step_count: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub record: RecordedStep,
    pub source_matches: bool,
    pub source_problem: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunView {
    pub run: RuntimeRun,
    pub steps: Vec<StepView>,
}
struct Lock(PathBuf);
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn locked(root: &Path) -> Result<Lock> {
    let dir = metadata_root(root)?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("trace.lock");
    reject_symlink(&path)?;
    for _ in 0..100 {
        match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(Lock(path)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                std::thread::sleep(std::time::Duration::from_millis(10))
            }
            Err(e) => return Err(e.into()),
        }
    }
    bail!("运行记录写入忙；异常残留 trace.lock 需人工检查")
}
fn load(root: &Path) -> Result<Store> {
    let path = metadata_root(root)?.join("runs.json");
    reject_symlink(&path)?;
    if !path.exists() {
        return Ok(Store::default());
    }
    let store: Store = serde_json::from_str(&read_limited_text(&path, MAX_METADATA_BYTES)?)
        .context("运行记录损坏，保留原文件，请先备份修复")?;
    ensure!(store.schema_version == 1, "不支持的运行记录版本");
    ensure!(store.runs.len() <= 50, "运行记录数量超限");
    Ok(store)
}
fn save(root: &Path, store: &Store) -> Result<()> {
    atomic_json(&metadata_root(root)?.join("runs.json"), store)
}
fn id(s: &str) -> Result<()> {
    ensure!(
        !s.is_empty()
            && s.len() <= 128
            && s.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
        "运行或步骤 ID 无效"
    );
    Ok(())
}
fn text(s: &str, max: usize) -> Result<()> {
    ensure!(
        !s.trim().is_empty() && s.chars().count() <= max,
        "运行记录文字为空或过长"
    );
    Ok(())
}
fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}
pub fn start(root: &Path, info: RunStart) -> Result<RuntimeRun> {
    let root = canonical_root(root)?;
    id(&info.id)?;
    text(&info.label, 200)?;
    text(&info.producer, 200)?;
    text(&info.summary, 2000)?;
    ensure!(
        ["application", "isolatedTest"].contains(&info.kind.as_str()),
        "必须明确实际应用或隔离测试来源"
    );
    let _lock = locked(&root)?;
    let mut store = load(&root)?;
    ensure!(
        store.runs.len() < 50,
        "已有 50 次运行，请备份并归档 runs.json 后再记录；不会自动删除历史"
    );
    ensure!(
        !store.runs.iter().any(|r| r.info.id == info.id),
        "运行 ID 已存在"
    );
    let run = RuntimeRun {
        info,
        status: "running".into(),
        started_at: now(),
        finished_at: None,
        revision: 0,
        steps: vec![],
    };
    store.runs.push(run.clone());
    save(&root, &store)?;
    Ok(run)
}
fn source(root: &Path, loc: &RuntimeLocation) -> Result<String> {
    ensure!(
        loc.start_line > 0 && loc.end_line >= loc.start_line && loc.end_line - loc.start_line < 200,
        "运行位置须为 1–200 行"
    );
    let path = normalize_relative(&loc.path)?;
    ensure!(
        !path.starts_with(".reader/") && !path.starts_with(".git/"),
        "不能把内部记录作为源码"
    );
    let file = read_file(root, &path)?;
    ensure!(
        file.hash == loc.file_hash,
        "源码已经变化，不能把当前内容冒充运行时源码"
    );
    let lines: Vec<_> = file.content.lines().collect();
    ensure!(loc.end_line <= lines.len(), "运行位置越过源码范围");
    let excerpt = lines[loc.start_line - 1..loc.end_line].join("\n");
    ensure!(excerpt.len() <= 16384, "源码片段过大");
    Ok(excerpt)
}
pub fn append(root: &Path, request: StepRequest) -> Result<RecordedStep> {
    let root = canonical_root(root)?;
    let step = &request.step;
    id(&request.run_id)?;
    id(&step.id)?;
    text(&step.title, 200)?;
    text(&step.result, 2000)?;
    text(&step.reason, 2000)?;
    ensure!(
        ["ok", "error", "skipped"].contains(&step.status.as_str()),
        "未知步骤状态"
    );
    for values in [&step.inputs, &step.outputs] {
        ensure!(values.len() <= 30, "每组最多记录 30 个显式变量");
        let mut names = HashSet::new();
        for value in values {
            text(&value.name, 100)?;
            ensure!(value.value.chars().count() <= 2000, "变量内容过长");
            ensure!(names.insert(&value.name), "变量名重复");
        }
    }
    let _lock = locked(&root)?;
    let mut store = load(&root)?;
    let run = store
        .runs
        .iter_mut()
        .find(|r| r.info.id == request.run_id)
        .context("运行不存在")?;
    ensure!(run.status == "running", "运行已结束，不允许追加或改写");
    ensure!(run.steps.len() < 200, "单次运行最多 200 个步骤");
    ensure!(
        !run.steps.iter().any(|s| s.observation.id == step.id),
        "步骤 ID 已存在"
    );
    let source_excerpt = source(&root, &step.location)?;
    let record = RecordedStep {
        sequence: run.steps.len() + 1,
        recorded_at: now(),
        observation: request.step,
        source_excerpt,
    };
    run.steps.push(record.clone());
    run.revision += 1;
    save(&root, &store)?;
    Ok(record)
}
pub fn finish(root: &Path, request: RunFinish) -> Result<RuntimeRun> {
    let root = canonical_root(root)?;
    id(&request.run_id)?;
    text(&request.summary, 2000)?;
    ensure!(
        ["completed", "failed", "cancelled"].contains(&request.status.as_str()),
        "未知运行结束状态"
    );
    let _lock = locked(&root)?;
    let mut store = load(&root)?;
    let run = store
        .runs
        .iter_mut()
        .find(|r| r.info.id == request.run_id)
        .context("运行不存在")?;
    ensure!(run.status == "running", "运行已结束，不允许改写");
    ensure!(
        request.status != "completed" || !run.steps.is_empty(),
        "没有步骤不能标记完成"
    );
    run.status = request.status;
    run.info.summary = request.summary;
    run.finished_at = Some(now());
    run.revision += 1;
    let result = run.clone();
    save(&root, &store)?;
    Ok(result)
}
pub fn list(root: &Path) -> Result<Vec<RunSummary>> {
    let root = canonical_root(root)?;
    Ok(load(&root)?
        .runs
        .into_iter()
        .rev()
        .map(|r| RunSummary {
            info: r.info,
            status: r.status,
            started_at: r.started_at,
            finished_at: r.finished_at,
            revision: r.revision,
            step_count: r.steps.len(),
        })
        .collect())
}
pub fn read(root: &Path, id: &str) -> Result<RunView> {
    let root = canonical_root(root)?;
    let run = load(&root)?
        .runs
        .into_iter()
        .find(|r| r.info.id == id)
        .context("运行不存在")?;
    let steps = run
        .steps
        .iter()
        .map(|record| {
            let check = source(&root, &record.observation.location);
            StepView {
                record: record.clone(),
                source_matches: check.is_ok(),
                source_problem: check.err().map(|e| e.to_string()),
            }
        })
        .collect();
    Ok(RunView { run, steps })
}
