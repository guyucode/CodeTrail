//! Versioned Agent declarations. Human review files are never written here.
use crate::*;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Delivery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overview: Option<ProjectOverview>,
    pub schema_version: u32,
    pub delivery_id: String,
    pub expected_revision: String,
    pub features: Vec<DeliveredFeature>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeliveredFeature {
    pub id: String,
    pub expected_digest: Option<String>,
    pub name: String,
    pub description: String,
    pub summary: String,
    pub evidence: Vec<Evidence>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Evidence {
    pub id: String,
    pub path: String,
    pub symbol_id: Option<String>,
    pub start_line: Option<u32>,
    pub end_line: Option<u32>,
    pub expected_file_hash: String,
    pub role: EvidenceRole,
    pub responsibility: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Store {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    overview: Option<ProjectOverview>,
    schema_version: u32,
    features: BTreeMap<String, StoredFeature>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredFeature {
    name: String,
    description: String,
    anchors: Vec<AnchorBinding>,
    delivery: AgentDeliveryInfo,
}

fn store_path(root: &Path) -> Result<PathBuf> {
    let path = metadata_root(root)?.join("agent.json");
    reject_symlink(&path)?;
    Ok(path)
}

fn load(root: &Path) -> Result<Store> {
    let path = store_path(root)?;
    if !path.exists() {
        return Ok(Store {
            schema_version: 1,
            ..Store::default()
        });
    }
    let store: Store = serde_json::from_str(&read_limited_text(&path, MAX_METADATA_BYTES)?)
        .context("Agent 交付文件损坏；原文件未修改")?;
    ensure!(store.schema_version == 1, "不支持的 Agent 协议版本");
    ensure!(store.features.len() <= 500, "Agent 功能数量超限");
    for (id, feature) in &store.features {
        validate_id(id)?;
        validate_id(&feature.delivery.delivery_id)?;
        validate_text(&feature.name, 200, "功能名称")?;
        validate_text(&feature.description, 10000, "功能说明")?;
        validate_text(&feature.delivery.summary, 4000, "交付说明")?;
        let mut ids = HashSet::new();
        ensure!(
            !feature.anchors.is_empty() && feature.anchors.len() <= 200,
            "证据数量无效"
        );
        for anchor in &feature.anchors {
            validate_id(&anchor.id)?;
            ensure!(ids.insert(&anchor.id), "证据 ID 重复");
            normalize_relative(&anchor.range.file)?;
            ensure!(
                anchor.range.start_line > 0 && anchor.range.end_line >= anchor.range.start_line,
                "证据范围无效"
            );
            ensure!(anchor.role.is_some(), "Agent 证据缺少职责角色");
            validate_text(&anchor.responsibility, 4000, "证据职责")?;
        }
    }
    Ok(store)
}

fn revision(store: &Store) -> Result<String> {
    Ok(digest(&serde_json::to_vec(store)?))
}

/// Non-source binary assets are intentionally outside the index, not incomplete evidence.
pub fn blocking_warnings(warnings: &[String]) -> Vec<&str> {
    warnings
        .iter()
        .filter(|warning| {
            !(warning.starts_with("跳过 ") && warning.ends_with("：二进制文件不支持代码阅读"))
        })
        .map(String::as_str)
        .collect()
}

/// Read-only inventory for the Agent: source hashes, stable IDs and update tokens.
pub fn inspect(root: &Path) -> Result<serde_json::Value> {
    let root = canonical_root(root)?;
    let project = open_project(&root)?;
    let files = project
        .files
        .iter()
        .map(|file| Ok((file.path.clone(), read_file(&root, &file.path)?.hash)))
        .collect::<Result<BTreeMap<_, _>>>()?;
    Ok(
        serde_json::json!({"schemaVersion":1,"revision":revision(&load(&root)?)?,
        "fileHashes":files,"project":project}),
    )
}

struct DeliveryLock(PathBuf);
impl Drop for DeliveryLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Upsert one atomic batch. Omitted features survive; no human state is accepted.
pub fn deliver(root: &Path, request: Delivery) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    ensure!(request.schema_version == 1, "不支持的 Agent 协议版本");
    validate_id(&request.delivery_id)?;
    ensure!(
        !request.features.is_empty() && request.features.len() <= 500,
        "交付功能数量无效"
    );
    let dir = metadata_root(&root)?;
    fs::create_dir_all(&dir)?;
    metadata_root(&root)?;
    let lock_path = dir.join("agent.lock");
    reject_symlink(&lock_path)?;
    let lock_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&lock_path)
        .context(
            "另一 Agent 正在交付；若上次异常退出，请确认无写入进程后清理 .reader/agent.lock",
        )?;
    let _lock = DeliveryLock(lock_path);
    let _lock_file = lock_file;
    let mut store = load(&root)?;
    ensure!(
        revision(&store)? == request.expected_revision,
        "交付版本已变化，请重新 agent inspect"
    );
    let snapshot = open_project(&root)?;
    ensure!(
        blocking_warnings(&snapshot.warnings).is_empty(),
        "项目有索引或标注警告，请先处理后交付：{:?}",
        snapshot.warnings
    );
    let mut touched = HashSet::new();
    let mut source_hashes = BTreeMap::new();
    for incoming in request.features {
        validate_id(&incoming.id)?;
        ensure!(touched.insert(incoming.id.clone()), "交付功能 ID 重复");
        validate_text(&incoming.name, 200, "功能名称")?;
        validate_text(&incoming.description, 10000, "功能说明")?;
        validate_text(&incoming.summary, 4000, "交付说明")?;
        let previous = snapshot.features.iter().find(|f| f.id == incoming.id);
        ensure!(
            incoming.expected_digest.as_deref() == previous.map(|f| f.current_digest.as_str()),
            "功能 {} 的版本摘要不匹配，请重新 inspect；新增功能使用 null",
            incoming.id
        );
        ensure!(
            !incoming.evidence.is_empty() && incoming.evidence.len() <= 200,
            "功能必须有 1–200 条证据"
        );
        let mut anchors = Vec::new();
        let mut ids = HashSet::new();
        let mut contents = HashMap::new();
        for evidence in incoming.evidence {
            validate_id(&evidence.id)?;
            ensure!(ids.insert(evidence.id.clone()), "功能内证据 ID 重复");
            validate_text(&evidence.responsibility, 4000, "证据职责")?;
            let file = read_file(&root, &evidence.path)?;
            ensure!(
                snapshot.files.iter().any(|f| f.path == file.path),
                "证据文件未被索引"
            );
            ensure!(
                file.hash == evidence.expected_file_hash,
                "源码摘要不匹配：{}",
                file.path
            );
            if let Some(previous_hash) = source_hashes.insert(file.path.clone(), file.hash) {
                ensure!(
                    previous_hash == evidence.expected_file_hash,
                    "交付期间源码变化：{}",
                    file.path
                );
            }
            let current_symbols = parse_symbols(&file.path, &file.content)?;
            let symbol = match evidence.symbol_id.as_deref() {
                Some(id) => {
                    let matches: Vec<_> = current_symbols
                        .iter()
                        .filter(|s| s.id == id && s.range.file == file.path)
                        .collect();
                    ensure!(matches.len() == 1, "符号缺失或有歧义：{id}");
                    Some(matches[0])
                }
                None => None,
            };
            let range = match (evidence.start_line, evidence.end_line, symbol) {
                (None, None, Some(symbol)) => symbol.range.clone(),
                (Some(start_line), Some(end_line), _) => CodeRange {
                    file: file.path.clone(),
                    start_line,
                    end_line,
                },
                _ => bail!("必须提供 symbolId 或完整起止行"),
            };
            ensure!(
                symbol.is_none_or(|s| contains_range(&s.range, &range)),
                "代码范围不属于指定符号"
            );
            let text = range_text(&file.content, &range).context("证据行范围无效")?;
            anchors.push(AnchorBinding {
                id: evidence.id,
                kind: if symbol.is_some_and(|s| s.range == range) {
                    AnchorKind::Symbol
                } else {
                    AnchorKind::Range
                },
                symbol_id: symbol.map(|s| s.id.clone()),
                symbol_name: symbol.map(|s| s.name.clone()),
                range,
                content_hash: digest(text.as_bytes()),
                status: AnchorStatus::Exact,
                note: String::new(),
                role: Some(evidence.role),
                responsibility: evidence.responsibility,
            });
            contents.insert(file.path, file.content);
        }
        let evidence_digest =
            declaration_digest(&incoming.name, &incoming.description, &anchors, &contents);
        store.features.insert(
            incoming.id,
            StoredFeature {
                name: incoming.name,
                description: incoming.description,
                anchors,
                delivery: AgentDeliveryInfo {
                    delivery_id: request.delivery_id.clone(),
                    summary: incoming.summary,
                    evidence_digest,
                    matches_source: true,
                },
            },
        );
    }
    ensure!(store.features.len() <= 500, "Agent 功能数量超限");
    for (path, hash) in source_hashes {
        ensure!(
            read_file(&root, &path)?.hash == hash,
            "交付期间源码变化：{path}"
        );
    }
    if let Some(overview) = request.overview {
        store.overview = Some(overview);
    }
    let mut targets: BTreeMap<String, HashSet<String>> = snapshot
        .features
        .iter()
        .map(|f| {
            (
                f.id.clone(),
                f.anchors.iter().map(|a| a.id.clone()).collect(),
            )
        })
        .collect();
    for (id, feature) in &store.features {
        targets.insert(
            id.clone(),
            feature.anchors.iter().map(|a| a.id.clone()).collect(),
        );
    }
    if let Some(overview) = &store.overview {
        validate_overview(overview, &targets)?;
    }
    atomic_json(&store_path(&root)?, &store)?;
    open_project(&root)
}

fn validate_text(value: &str, max: usize, label: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.chars().count() <= max,
        "{label}不能为空或超长"
    );
    Ok(())
}

pub(crate) fn merge(root: &Path, mut features: Vec<Feature>) -> Result<Vec<Feature>> {
    for (id, declaration) in load(root)?.features {
        let existing = features
            .iter()
            .position(|f| f.id == id)
            .map(|i| features.remove(i));
        let mut anchors = declaration.anchors;
        // Notes belong to humans, never to a delivery request.
        if let Some(previous) = &existing {
            for anchor in &mut anchors {
                anchor.note = previous
                    .anchors
                    .iter()
                    .find(|a| a.id == anchor.id)
                    .map(|a| a.note.clone())
                    .unwrap_or_default();
            }
        }
        let mut feature = existing.unwrap_or_else(|| Feature {
            id: id.clone(),
            name: String::new(),
            description: String::new(),
            color: "#67c9f3".into(),
            parent_id: None,
            anchors: Vec::new(),
            review_state: ReviewState::Unread,
            reviewed_at: None,
            reviewed_digest: None,
            current_digest: String::new(),
            agent_delivery: None,
        });
        feature.name = declaration.name;
        feature.description = declaration.description;
        feature.anchors = anchors;
        feature.agent_delivery = Some(declaration.delivery);
        features.push(feature);
    }
    features.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(features)
}

fn declaration_digest(
    name: &str,
    description: &str,
    anchors: &[AnchorBinding],
    contents: &HashMap<String, String>,
) -> String {
    let evidence: BTreeMap<_, _> = anchors
        .iter()
        .map(|a| {
            let current = contents
                .get(&a.range.file)
                .and_then(|s| range_text(s, &a.range))
                .map(|s| digest(s.as_bytes()));
            (
                &a.id,
                (
                    &a.kind,
                    &a.range.file,
                    &a.symbol_id,
                    &a.role,
                    &a.responsibility,
                    current,
                ),
            )
        })
        .collect();
    digest(&serde_json::to_vec(&(name, description, evidence)).unwrap_or_default())
}

pub(crate) fn check_delivery(feature: &mut Feature, contents: &HashMap<String, String>) {
    let current = declaration_digest(
        &feature.name,
        &feature.description,
        &feature.anchors,
        contents,
    );
    let valid = feature
        .anchors
        .iter()
        .all(|a| matches!(a.status, AnchorStatus::Exact | AnchorStatus::Moved));
    if let Some(delivery) = &mut feature.agent_delivery {
        delivery.matches_source = valid && current == delivery.evidence_digest;
    }
}

/// Validate guide identity and links against the effective feature/evidence set.
fn validate_overview(
    overview: &ProjectOverview,
    targets: &BTreeMap<String, HashSet<String>>,
) -> Result<()> {
    validate_text(&overview.summary, 4000, "项目概述")?;
    ensure!(
        !overview.modules.is_empty() && overview.modules.len() <= 20,
        "模块数量须为 1–20"
    );
    ensure!(
        !overview.paths.is_empty() && overview.paths.len() <= 10,
        "运行路径数量须为 1–10"
    );
    let mut modules = HashSet::new();
    for module in &overview.modules {
        validate_id(&module.id)?;
        ensure!(modules.insert(&module.id), "模块 ID 重复");
        validate_text(&module.name, 100, "模块名称")?;
        validate_text(&module.responsibility, 1000, "模块职责")?;
    }
    let mut paths = HashSet::new();
    for path in &overview.paths {
        validate_id(&path.id)?;
        ensure!(paths.insert(&path.id), "路径 ID 重复");
        validate_text(&path.name, 100, "路径名称")?;
        validate_text(&path.description, 2000, "路径说明")?;
        ensure!(
            !path.steps.is_empty() && path.steps.len() <= 20,
            "路径步骤数量须为 1–20"
        );
        let mut steps = HashSet::new();
        for step in &path.steps {
            validate_id(&step.id)?;
            ensure!(steps.insert(&step.id), "路径内步骤 ID 重复");
            validate_text(&step.title, 100, "步骤标题")?;
            validate_text(&step.description, 2000, "步骤说明")?;
            ensure!(
                modules.contains(&step.module_id),
                "步骤引用未知模块：{}",
                step.module_id
            );
            ensure!(
                targets
                    .get(&step.feature_id)
                    .is_some_and(|ids| ids.contains(&step.evidence_id)),
                "步骤引用无效功能或证据：{} / {}",
                step.feature_id,
                step.evidence_id
            );
        }
    }
    Ok(())
}

/// Invalid guide metadata is reported as an index warning; source reading still works.
pub(crate) fn project_overview(
    root: &Path,
    features: &[Feature],
) -> Result<Option<ProjectOverview>> {
    let overview = load(root)?.overview;
    if let Some(guide) = &overview {
        let targets = features
            .iter()
            .map(|f| {
                (
                    f.id.clone(),
                    f.anchors.iter().map(|a| a.id.clone()).collect(),
                )
            })
            .collect();
        validate_overview(guide, &targets)?;
    }
    Ok(overview)
}
