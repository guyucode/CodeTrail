//! Local, source-read-only feature reading and review engine.
pub mod agent;
pub mod collaboration;
pub mod git;
pub mod journal;
pub mod parse;
pub mod relations;
pub mod semantic;
pub mod trace;
pub mod types;
pub use parse::parse_symbols;
pub use types::*;

use anyhow::{bail, ensure, Context, Result};
use ignore::WalkBuilder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_FILES: usize = 10_000;
const MAX_METADATA_BYTES: u64 = 2 * 1024 * 1024;
static WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct IndexHeader {
    schema_version: u32,
}

struct Scan {
    files: Vec<FileEntry>,
    symbols: Vec<Symbol>,
    contents: HashMap<String, String>,
    warnings: Vec<String>,
}

pub fn open_project(root: &Path) -> Result<ProjectSnapshot> {
    let root = canonical_root(root)?;
    let mut scan = scan_repository(&root)?;
    let mut features = load_features(&root, &mut scan.warnings)?;
    for feature in &mut features {
        for anchor in &mut feature.anchors {
            if anchor.role.is_some()
                && anchor
                    .symbol_id
                    .as_ref()
                    .is_some_and(|id| scan.symbols.iter().filter(|s| &s.id == id).count() > 1)
            {
                anchor.status = AnchorStatus::Ambiguous;
            } else {
                resolve_anchor(anchor, &scan.symbols, &scan.contents);
            }
        }
        feature.current_digest = review_digest(feature, &scan.contents);
        agent::check_delivery(feature, &scan.contents);
        if feature.review_state == ReviewState::Verified {
            let invalid = feature.anchors.is_empty()
                || feature
                    .anchors
                    .iter()
                    .any(|a| matches!(a.status, AnchorStatus::Missing | AnchorStatus::Ambiguous));
            if invalid
                || feature.reviewed_digest.as_deref() != Some(feature.current_digest.as_str())
            {
                feature.review_state = ReviewState::NeedsReview;
            }
        }
    }
    let overview = match agent::project_overview(&root, &features) {
        Ok(overview) => overview,
        Err(error) => {
            scan.warnings.push(format!("项目概览未加载：{error}"));
            None
        }
    };
    Ok(ProjectSnapshot {
        overview,
        name: root
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string()),
        root: root.to_string_lossy().into_owned(),
        git: git::info(&root),
        files: scan.files,
        symbols: scan.symbols,
        features,
        warnings: scan.warnings,
    })
}

pub fn read_file(root: &Path, path: &str) -> Result<FileContent> {
    let root = canonical_root(root)?;
    let normalized = normalize_relative(path)?;
    let full = safe_source_path(&root, &normalized)?;
    let content = read_limited_text(&full, MAX_FILE_BYTES)?;
    Ok(FileContent {
        path: normalized.clone(),
        language: parse::language_for(&normalized).to_owned(),
        hash: digest(content.as_bytes()),
        content,
    })
}

pub fn save_feature(root: &Path, draft: FeatureDraft) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    ensure!(
        !draft.name.trim().is_empty() && draft.name.chars().count() <= 200,
        "功能名称不能为空，且不能超过 200 字符"
    );
    ensure!(draft.description.chars().count() <= 10_000, "功能说明过长");
    ensure!(
        draft.color.len() == 7
            && draft.color.starts_with('#')
            && draft.color[1..].bytes().all(|b| b.is_ascii_hexdigit()),
        "颜色必须是 #RRGGBB"
    );
    let mut warnings = Vec::new();
    let features = load_features(&root, &mut warnings)?;
    let is_update = draft.id.is_some();
    let id = draft.id.unwrap_or_else(new_id);
    validate_id(&id)?;
    validate_parent(&id, draft.parent_id.as_deref(), &features)?;
    let existing = features.into_iter().find(|f| f.id == id);
    ensure!(
        !is_update || existing.is_some(),
        "功能不存在或标注损坏；请刷新后重试，原文件未覆盖"
    );
    ensure!(
        existing.as_ref().is_none_or(|f| f.agent_delivery.is_none()),
        "Agent 交付功能请通过 agent deliver 更新；人工审阅独立保存"
    );
    let mut feature = existing.unwrap_or_else(|| Feature {
        id: id.clone(),
        name: String::new(),
        description: String::new(),
        color: draft.color.clone(),
        parent_id: None,
        anchors: Vec::new(),
        review_state: ReviewState::Unread,
        reviewed_at: None,
        reviewed_digest: None,
        current_digest: String::new(),
        agent_delivery: None,
    });
    if feature.name != draft.name.trim() || feature.description != draft.description {
        invalidate_review(&mut feature);
    }
    feature.name = draft.name.trim().to_owned();
    feature.description = draft.description;
    feature.color = draft.color;
    feature.parent_id = draft.parent_id;
    persist_feature(&root, &feature)?;
    open_project(&root)
}

pub fn delete_feature(root: &Path, feature_id: &str) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    validate_id(feature_id)?;
    let features = load_features(&root, &mut Vec::new())?;
    ensure!(features.iter().any(|f| f.id == feature_id), "功能不存在");
    ensure!(
        !features
            .iter()
            .any(|f| f.id == feature_id && f.agent_delivery.is_some()),
        "不能删除 Agent 交付功能；请通过 Agent 更新协议"
    );
    ensure!(
        !features
            .iter()
            .any(|f| f.parent_id.as_deref() == Some(feature_id)),
        "请先移动或删除子功能"
    );
    let path = metadata_file(&root, feature_id)?;
    reject_symlink(&path)?;
    fs::remove_file(path).context("删除功能标注失败")?;
    open_project(&root)
}

pub fn bind_anchor(root: &Path, request: BindRequest) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    let snapshot = open_project(&root)?;
    let mut feature = snapshot
        .features
        .iter()
        .find(|f| f.id == request.feature_id)
        .cloned()
        .context("功能不存在或标注文件损坏")?;
    ensure!(
        feature.agent_delivery.is_none(),
        "Agent 代码关联请通过 agent deliver 更新"
    );
    let file = read_file(&root, &request.path)?;
    ensure!(
        snapshot.files.iter().any(|f| f.path == file.path),
        "文件不在当前索引中，无法绑定"
    );
    let range = CodeRange {
        file: file.path.clone(),
        start_line: request.start_line,
        end_line: request.end_line,
    };
    let text = range_text(&file.content, &range).context("请选择有效的代码行范围")?;
    let symbol = match request.symbol_id.as_deref() {
        Some(id) => {
            let found = snapshot
                .symbols
                .iter()
                .find(|s| s.id == id)
                .context("符号已变化，请刷新后重新选择")?;
            ensure!(
                found.range.file == file.path
                    && range.start_line >= found.range.start_line
                    && range.end_line <= found.range.end_line,
                "选择范围不属于指定符号"
            );
            Some(found)
        }
        None => snapshot
            .symbols
            .iter()
            .filter(|s| contains_range(&s.range, &range))
            .min_by_key(|s| s.range.end_line - s.range.start_line),
    };
    let id = request.anchor_id.clone().unwrap_or_else(new_id);
    validate_id(&id)?;
    if request.anchor_id.is_some() {
        ensure!(
            feature.anchors.iter().any(|a| a.id == id),
            "要重新绑定的锚点不存在"
        );
    }
    ensure!(
        !feature
            .anchors
            .iter()
            .any(|a| a.id != id && a.range == range),
        "该代码范围已绑定到当前功能"
    );
    let note = request.note.unwrap_or_default();
    ensure!(note.chars().count() <= 4000, "标注说明过长");
    let anchor = AnchorBinding {
        id: id.clone(),
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
        note,
        role: None,
        responsibility: String::new(),
    };
    if let Some(existing) = feature.anchors.iter_mut().find(|a| a.id == id) {
        *existing = anchor;
    } else {
        feature.anchors.push(anchor);
    }
    invalidate_review(&mut feature);
    persist_feature(&root, &feature)?;
    open_project(&root)
}

pub fn remove_anchor(root: &Path, feature_id: &str, anchor_id: &str) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    let mut feature = load_feature(&root, feature_id)?;
    ensure!(
        feature.agent_delivery.is_none(),
        "Agent 代码关联请通过 agent deliver 更新"
    );
    ensure!(
        feature.anchors.iter().any(|a| a.id == anchor_id),
        "锚点不存在"
    );
    feature.anchors.retain(|a| a.id != anchor_id);
    invalidate_review(&mut feature);
    persist_feature(&root, &feature)?;
    open_project(&root)
}

pub fn set_review(
    root: &Path,
    feature_id: &str,
    state: ReviewState,
    expected_digest: Option<&str>,
) -> Result<ProjectSnapshot> {
    let _guard = mutation_guard()?;
    let root = canonical_root(root)?;
    let snapshot = open_project(&root)?;
    let mut feature = snapshot
        .features
        .into_iter()
        .find(|f| f.id == feature_id)
        .context("功能不存在")?;
    if state == ReviewState::Verified {
        ensure!(
            expected_digest == Some(feature.current_digest.as_str()),
            "代码或标注在阅读后发生变化，或未提交阅读快照；请刷新并重新审阅后再确认"
        );
        ensure!(!feature.anchors.is_empty(), "请先绑定代码，再确认审阅");
        ensure!(
            !feature
                .anchors
                .iter()
                .any(|a| matches!(a.status, AnchorStatus::Missing | AnchorStatus::Ambiguous)),
            "存在失效或歧义锚点，请先重新绑定"
        );
        let mut contents = HashMap::new();
        for anchor in &mut feature.anchors {
            let file = read_file(&root, &anchor.range.file)?;
            let text =
                range_text(&file.content, &anchor.range).context("代码范围已变化，请先重新绑定")?;
            anchor.content_hash = digest(text.as_bytes());
            anchor.status = AnchorStatus::Exact;
            contents.insert(file.path, file.content);
        }
        let current_digest = review_digest(&feature, &contents);
        ensure!(
            expected_digest == Some(current_digest.as_str()),
            "确认期间源码发生变化；请刷新并重新审阅"
        );
        feature.current_digest = current_digest.clone();
        feature.reviewed_digest = Some(current_digest);
        feature.reviewed_at = Some(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)?
                .as_secs()
                .to_string(),
        );
    } else if state != ReviewState::NeedsReview {
        feature.reviewed_digest = None;
        feature.reviewed_at = None;
    }
    feature.review_state = state;
    persist_feature(&root, &feature)?;
    open_project(&root)
}

pub fn export_review(root: &Path) -> Result<String> {
    let project = open_project(root)?;
    let mut out = format!("# {} · 代码阅读记录\n\n源码快照：`{}`\n\n本报告记录人工阅读状态，不等同于测试通过或行为正确性证明。\n", markdown_text(&project.name), project.git.head.as_deref().unwrap_or("无 Git 提交"));
    for feature in project.features {
        let state = match feature.review_state {
            ReviewState::Unread => "未读",
            ReviewState::Reading => "阅读中",
            ReviewState::Verified => "已审阅",
            ReviewState::NeedsReview => "需要重新审阅",
        };
        out.push_str(&format!(
            "\n## {} · {}\n\n{}\n\n",
            markdown_text(&feature.name),
            state,
            feature.description
        ));
        if let Some(delivery) = &feature.agent_delivery {
            out.push_str(&format!(
                "功能 ID：`{}` · Agent 交付：`{}` · 证据{}\n\n{}\n\n",
                markdown_text(&feature.id),
                markdown_text(&delivery.delivery_id),
                if delivery.matches_source {
                    "与源码一致"
                } else {
                    "已变化"
                },
                markdown_text(&delivery.summary)
            ));
        }
        for a in feature.anchors {
            let status = match a.status {
                AnchorStatus::Exact => "已定位",
                AnchorStatus::Moved => "位置已移动",
                AnchorStatus::Changed => "内容已变化",
                AnchorStatus::Missing => "失效",
                AnchorStatus::Ambiguous => "存在歧义",
            };
            out.push_str(&format!(
                "- `{}` L{}–L{} · {}{}\n",
                markdown_text(&a.range.file),
                a.range.start_line,
                a.range.end_line,
                status,
                if a.note.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", markdown_text(&a.note))
                }
            ));
            if let Some(role) = &a.role {
                let role = match role {
                    EvidenceRole::Entry => "入口",
                    EvidenceRole::Implementation => "实现",
                    EvidenceRole::Constraint => "约束",
                    EvidenceRole::ErrorHandling => "异常处理",
                    EvidenceRole::Test => "测试",
                };
                out.push_str(&format!(
                    "  - {role}：{}\n",
                    markdown_text(&a.responsibility)
                ));
            }
        }
    }
    if !project.warnings.is_empty() {
        out.push_str("\n## 索引提示\n\n");
        for warning in project.warnings {
            out.push_str(&format!("- {}\n", markdown_text(&warning)));
        }
    }
    Ok(out)
}

fn canonical_root(root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().context("仓库目录不存在或无法访问")?;
    ensure!(root.is_dir(), "请选择一个目录");
    Ok(root)
}

fn normalize_relative(path: &str) -> Result<String> {
    ensure!(
        !path.is_empty() && !path.contains('\\') && !path.contains('\0'),
        "文件路径无效"
    );
    let mut parts = Vec::new();
    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_str().context("仅支持 UTF-8 文件名")?;
                ensure!(
                    part != ".git" && part != ".reader" && !part.contains(':'),
                    "不能访问仓库内部元数据"
                );
                parts.push(part);
            }
            Component::CurDir => {}
            _ => bail!("文件路径必须位于选定仓库内"),
        }
    }
    ensure!(!parts.is_empty(), "请选择文件");
    Ok(parts.join("/"))
}

fn safe_source_path(root: &Path, relative: &str) -> Result<PathBuf> {
    let mut full = root.to_path_buf();
    for component in Path::new(relative).components() {
        full.push(component);
        reject_symlink(&full)?;
    }
    let full = full.canonicalize().context("文件不存在")?;
    ensure!(
        full.starts_with(root) && full.is_file(),
        "文件不在仓库内或不是普通文件"
    );
    Ok(full)
}

fn reject_symlink(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(meta) => ensure!(
            !meta.file_type().is_symlink(),
            "为保持只读边界，拒绝符号链接：{}",
            path.display()
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("无法检查路径 {}", path.display())),
    }
    Ok(())
}

fn read_limited_text(path: &Path, limit: u64) -> Result<String> {
    let file = fs::File::open(path).with_context(|| format!("无法读取 {}", path.display()))?;
    let metadata = file.metadata()?;
    ensure!(
        metadata.is_file() && metadata.len() <= limit,
        "文件超过 {} MiB 限制或不是普通文件",
        limit / 1024 / 1024
    );
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(bytes.len() as u64 <= limit, "读取时文件超过大小限制");
    ensure!(!bytes.contains(&0), "二进制文件不支持代码阅读");
    String::from_utf8(bytes).context("文件不是有效 UTF-8 文本")
}

fn scan_repository(root: &Path) -> Result<Scan> {
    let mut scan = Scan {
        files: Vec::new(),
        symbols: Vec::new(),
        contents: HashMap::new(),
        warnings: Vec::new(),
    };
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true)
        .git_global(false)
        .git_exclude(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| {
            !entry.file_type().is_some_and(|t| t.is_dir())
                || !matches!(
                    entry.file_name().to_str().unwrap_or(""),
                    ".git"
                        | ".reader"
                        | "node_modules"
                        | "target"
                        | "dist"
                        | "build"
                        | "coverage"
                        | "__pycache__"
                        | ".venv"
                        | "venv"
                )
        });
    let mut paths = Vec::new();
    for entry in builder.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                scan.warnings.push(format!("目录扫描提示：{e}"));
                continue;
            }
        };
        if entry.file_type().is_some_and(|t| t.is_file()) {
            if paths.len() >= MAX_FILES {
                scan.warnings
                    .push("目录超过 10,000 个候选文件，仅索引部分文件；请选择更小目录".into());
                break;
            }
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_str()
                .context("仅支持 UTF-8 路径")?
                .replace('\\', "/");
            paths.push(relative);
        }
    }
    paths.sort();
    let mut total = 0;
    for relative in paths {
        if scan.files.len() >= MAX_FILES || total >= MAX_INDEX_BYTES {
            scan.warnings
                .push("索引达到 10,000 文件 / 64 MiB 上限，请选择更小目录".into());
            break;
        }
        let path = match normalize_relative(&relative).and_then(|p| safe_source_path(root, &p)) {
            Ok(path) => path,
            Err(err) => {
                scan.warnings.push(format!("跳过 {relative}：{err}"));
                continue;
            }
        };
        let content = match read_limited_text(&path, MAX_FILE_BYTES) {
            Ok(content) => content,
            Err(err) => {
                scan.warnings.push(format!("跳过 {relative}：{err}"));
                continue;
            }
        };
        total += content.len() as u64;
        let (symbols, partial) = parse::parse_with_status(&relative, &content)?;
        if partial {
            scan.warnings
                .push(format!("{relative} 含语法错误；符号索引可能不完整"));
        }
        scan.symbols.extend(symbols);
        scan.files.push(FileEntry {
            path: relative.clone(),
            language: parse::language_for(&relative).to_owned(),
            size: content.len() as u64,
        });
        scan.contents.insert(relative, content);
    }
    scan.symbols.sort_by(|a, b| {
        (&a.range.file, a.range.start_line, &a.id).cmp(&(&b.range.file, b.range.start_line, &b.id))
    });
    Ok(scan)
}

fn metadata_root(root: &Path) -> Result<PathBuf> {
    let dir = root.join(".reader");
    reject_symlink(&dir)?;
    if dir.exists() {
        ensure!(dir.is_dir(), ".reader 必须是目录");
    }
    let features = dir.join("features");
    reject_symlink(&features)?;
    if features.exists() {
        ensure!(features.is_dir(), ".reader/features 必须是目录");
    }
    Ok(dir)
}

fn load_features(root: &Path, warnings: &mut Vec<String>) -> Result<Vec<Feature>> {
    let dir = metadata_root(root)?;
    let index = dir.join("index.json");
    reject_symlink(&index)?;
    if index.exists() {
        let header: IndexHeader =
            serde_json::from_str(&read_limited_text(&index, MAX_METADATA_BYTES)?)
                .context("阅读索引格式损坏，原文件未修改")?;
        ensure!(
            header.schema_version == SCHEMA_VERSION,
            "不支持的阅读索引版本 {}（当前 {}）",
            header.schema_version,
            SCHEMA_VERSION
        );
    }
    let feature_dir = dir.join("features");
    if !feature_dir.exists() {
        return agent::merge(root, Vec::new());
    }
    let mut paths = fs::read_dir(feature_dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    let mut features = Vec::new();
    let mut seen = HashSet::new();
    for path in paths {
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let parsed = (|| -> Result<Feature> {
            reject_symlink(&path)?;
            let feature: Feature =
                serde_json::from_str(&read_limited_text(&path, MAX_METADATA_BYTES)?)?;
            validate_id(&feature.id)?;
            ensure!(
                path.file_stem().and_then(|s| s.to_str()) == Some(&feature.id),
                "文件名与功能 id 不一致"
            );
            ensure!(seen.insert(feature.id.clone()), "功能 id 重复");
            let mut anchor_ids = HashSet::new();
            for anchor in &feature.anchors {
                validate_id(&anchor.id)?;
                ensure!(anchor_ids.insert(&anchor.id), "锚点 id 重复");
                normalize_relative(&anchor.range.file)?;
                ensure!(
                    anchor.range.start_line > 0 && anchor.range.end_line >= anchor.range.start_line,
                    "锚点范围无效"
                );
            }
            Ok(feature)
        })();
        match parsed {
            Ok(feature) => features.push(feature),
            Err(err) => warnings.push(format!(
                "未加载标注 {}：{err}；原文件已保留",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    for feature in &features {
        if let Err(err) = validate_parent(&feature.id, feature.parent_id.as_deref(), &features) {
            warnings.push(format!("功能 {} 层级异常：{err}", feature.name));
        }
    }
    features.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    agent::merge(root, features)
}

fn load_feature(root: &Path, id: &str) -> Result<Feature> {
    validate_id(id)?;
    load_features(root, &mut Vec::new())?
        .into_iter()
        .find(|f| f.id == id)
        .context("功能不存在或标注文件损坏")
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
        "标注 id 无效"
    );
    Ok(())
}

fn validate_parent(id: &str, parent: Option<&str>, features: &[Feature]) -> Result<()> {
    let mut cursor = parent;
    let mut visited = HashSet::new();
    while let Some(parent) = cursor {
        ensure!(
            parent != id && visited.insert(parent),
            "功能层级不能包含循环"
        );
        let feature = features
            .iter()
            .find(|f| f.id == parent)
            .context("父功能不存在")?;
        cursor = feature.parent_id.as_deref();
    }
    Ok(())
}

fn metadata_file(root: &Path, id: &str) -> Result<PathBuf> {
    validate_id(id)?;
    Ok(metadata_root(root)?
        .join("features")
        .join(format!("{id}.json")))
}

fn persist_feature(root: &Path, feature: &Feature) -> Result<()> {
    let dir = metadata_root(root)?;
    fs::create_dir_all(dir.join("features"))?;
    metadata_root(root)?;
    let index = dir.join("index.json");
    if !index.exists() {
        atomic_json(
            &index,
            &IndexHeader {
                schema_version: SCHEMA_VERSION,
            },
        )?;
    }
    atomic_json(&metadata_file(root, &feature.id)?, feature)
}

fn atomic_json(path: &Path, value: &impl Serialize) -> Result<()> {
    reject_symlink(path)?;
    let bytes = serde_json::to_vec_pretty(value)?;
    ensure!(
        bytes.len() as u64 <= MAX_METADATA_BYTES,
        "标注内容超出大小限制"
    );
    let temp = path.with_file_name(format!(".{}.tmp", new_id()));
    let result = (|| -> Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)?;
        file.write_all(&bytes)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
        reject_symlink(path)?;
        fs::rename(&temp, path).context("原子保存阅读标注失败")?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}

fn mutation_guard() -> Result<std::sync::MutexGuard<'static, ()>> {
    WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("标注写入锁异常，请重启应用"))
}

fn new_id() -> String {
    Uuid::new_v4().simple().to_string()
}
fn digest(content: &[u8]) -> String {
    format!("{:x}", Sha256::digest(content))
}
fn markdown_text(text: &str) -> String {
    text.replace(['\n', '\r', '`'], " ")
}
fn invalidate_review(feature: &mut Feature) {
    if feature.review_state == ReviewState::Verified {
        feature.review_state = ReviewState::NeedsReview;
    }
}

/// Original line endings are preserved. End lines are inclusive.
fn range_text<'a>(content: &'a str, range: &CodeRange) -> Option<&'a str> {
    if range.start_line == 0 || range.end_line < range.start_line {
        return None;
    }
    let offsets = line_offsets(content);
    let start = *offsets.get(range.start_line as usize - 1)?;
    let end = offsets
        .get(range.end_line as usize)
        .copied()
        .unwrap_or(content.len());
    if range.end_line as usize > offsets.len() {
        return None;
    }
    content.get(start..end)
}

fn line_offsets(content: &str) -> Vec<usize> {
    let mut offsets = vec![0];
    for (index, byte) in content.bytes().enumerate() {
        if byte == b'\n' {
            offsets.push(index + 1);
        }
    }
    offsets
}

fn contains_range(owner: &CodeRange, range: &CodeRange) -> bool {
    owner.file == range.file
        && owner.start_line <= range.start_line
        && owner.end_line >= range.end_line
}

fn resolve_anchor(
    anchor: &mut AnchorBinding,
    symbols: &[Symbol],
    contents: &HashMap<String, String>,
) {
    if anchor.kind == AnchorKind::Symbol {
        let Some(symbol) = anchor
            .symbol_id
            .as_ref()
            .and_then(|id| symbols.iter().find(|s| &s.id == id))
        else {
            anchor.status = AnchorStatus::Missing;
            return;
        };
        let Some(content) = contents
            .get(&symbol.range.file)
            .and_then(|c| range_text(c, &symbol.range))
        else {
            anchor.status = AnchorStatus::Missing;
            return;
        };
        anchor.status = if digest(content.as_bytes()) != anchor.content_hash {
            AnchorStatus::Changed
        } else if anchor.range != symbol.range {
            AnchorStatus::Moved
        } else {
            AnchorStatus::Exact
        };
        anchor.range = symbol.range.clone();
        return;
    }
    let Some(content) = contents.get(&anchor.range.file) else {
        anchor.status = AnchorStatus::Missing;
        return;
    };
    let owner = if let Some(id) = anchor.symbol_id.as_ref() {
        let Some(symbol) = symbols
            .iter()
            .find(|s| &s.id == id && s.range.file == anchor.range.file)
        else {
            anchor.status = AnchorStatus::Missing;
            return;
        };
        Some(&symbol.range)
    } else {
        None
    };
    let original_in_owner = owner.is_none_or(|scope| contains_range(scope, &anchor.range));
    if original_in_owner
        && range_text(content, &anchor.range)
            .is_some_and(|text| digest(text.as_bytes()) == anchor.content_hash)
    {
        anchor.status = AnchorStatus::Exact;
        return;
    }
    let length = (anchor.range.end_line - anchor.range.start_line + 1) as usize;
    let offsets = line_offsets(content);
    let mut candidates = Vec::new();
    let first = owner.map_or(0, |scope| scope.start_line as usize - 1);
    let limit = owner
        .map_or(offsets.len(), |scope| scope.end_line as usize)
        .min(offsets.len());
    // Bound worst-case relocation work for long selections. Never silently guess.
    if first + length <= limit && content.len().saturating_mul(length) <= 32 * 1024 * 1024 {
        for start in first..=limit - length {
            let end = offsets
                .get(start + length)
                .copied()
                .unwrap_or(content.len());
            if digest(&content.as_bytes()[offsets[start]..end]) == anchor.content_hash {
                candidates.push(start);
                if candidates.len() > 1 {
                    break;
                }
            }
        }
    }
    match candidates.as_slice() {
        [start] => {
            anchor.range.start_line = *start as u32 + 1;
            anchor.range.end_line = (*start + length) as u32;
            anchor.status = AnchorStatus::Moved;
        }
        [] => {
            anchor.status = if original_in_owner && range_text(content, &anchor.range).is_some() {
                AnchorStatus::Changed
            } else {
                AnchorStatus::Missing
            }
        }
        _ => anchor.status = AnchorStatus::Ambiguous,
    }
}

fn review_digest(feature: &Feature, contents: &HashMap<String, String>) -> String {
    let mut evidence = BTreeMap::new();
    for anchor in &feature.anchors {
        let current = contents
            .get(&anchor.range.file)
            .and_then(|s| range_text(s, &anchor.range))
            .map(|s| digest(s.as_bytes()))
            .unwrap_or_else(|| "missing".to_owned());
        evidence.insert(
            &anchor.id,
            (
                &anchor.kind,
                &anchor.range.file,
                &anchor.symbol_id,
                &anchor.note,
                current,
            ),
        );
    }
    let base = (&feature.name, &feature.description, evidence);
    let roles: BTreeMap<_, _> = feature
        .anchors
        .iter()
        .filter(|a| a.role.is_some())
        .map(|a| (&a.id, (&a.role, &a.responsibility)))
        .collect();
    if !roles.is_empty() {
        return digest(&serde_json::to_vec(&(base, roles)).unwrap_or_default());
    }
    digest(serde_json::to_vec(&base).unwrap_or_default().as_slice())
}
