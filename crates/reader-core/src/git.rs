//! Read-only Git snapshots and line-based change evidence.
//!
//! Worktree content is read directly. In particular, we do not run `git diff`
//! against the worktree: repository-configured clean filters can execute code.
//! Snapshot/index-only diffs are used solely for Git's rename hints.

use crate::types::*;
use anyhow::{anyhow, bail, Context, Result};
use similar::{ChangeTag, TextDiff};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Output};

const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_COMPARED_FILES: usize = 2000;
const MAX_COMPARED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Clone)]
struct TreeEntry {
    oid: String,
    mode: String,
}

#[derive(Clone)]
struct Record {
    path: String,
    old_path: Option<String>,
    status: ChangeStatus,
    old: String,
    new: String,
}

struct Comparison {
    records: Vec<Record>,
    warnings: Vec<String>,
    base_oid: Option<String>,
    head_oid: Option<String>,
}

fn git(root: &Path, args: &[&str]) -> Result<Output> {
    let mut command = Command::new("git");
    command
        .arg("--no-pager")
        .args([
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
        ])
        .args(["-c", "core.hooksPath=/dev/null", "-c", "diff.external="])
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("LC_ALL", "C");
    for key in [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_INDEX_FILE",
        "GIT_OBJECT_DIRECTORY",
        "GIT_ALTERNATE_OBJECT_DIRECTORIES",
        "GIT_CONFIG_COUNT",
        "GIT_EXTERNAL_DIFF",
        "GIT_DIFF_OPTS",
    ] {
        command.env_remove(key);
    }
    command.output().context("无法运行 Git，请确认已安装 Git")
}

fn git_ok(root: &Path, args: &[&str]) -> Result<Vec<u8>> {
    let output = git(root, args)?;
    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        bail!("Git 读取失败：{}", error.trim());
    }
    Ok(output.stdout)
}

fn git_text(root: &Path, args: &[&str]) -> Result<String> {
    Ok(String::from_utf8(git_ok(root, args)?)?.trim().to_owned())
}

/// Best-effort repository metadata, also usable for non-Git directories.
pub fn info(root: &Path) -> GitInfo {
    let available =
        git_text(root, &["rev-parse", "--is-inside-work-tree"]).is_ok_and(|value| value == "true");
    if !available {
        return GitInfo {
            available: false,
            branch: None,
            head: None,
        };
    }
    let branch = git_text(root, &["symbolic-ref", "--quiet", "--short", "HEAD"]).ok();
    let head = git_text(root, &["rev-parse", "--verify", "--quiet", "HEAD^{commit}"]).ok();
    GitInfo {
        available,
        branch,
        head,
    }
}

fn comparison_root(root: &Path) -> Result<PathBuf> {
    let root = root.canonicalize().context("项目目录不存在")?;
    let top = git_text(&root, &["rev-parse", "--show-toplevel"])?;
    let top = Path::new(&top)
        .canonicalize()
        .context("无法定位 Git 仓库根目录")?;
    if root != top {
        bail!(
            "Git 对比请打开仓库根目录：{}。当前子目录仍可正常阅读和标注。",
            top.display()
        );
    }
    Ok(root)
}

fn verify_commit(root: &Path, revision: &str, allow_unborn: bool) -> Result<Option<String>> {
    if revision.is_empty()
        || revision.len() > 512
        || revision.starts_with('-')
        || revision.chars().any(char::is_control)
        || revision == "WORKTREE"
    {
        bail!("无效的 Git 版本：{revision:?}");
    }
    let expression = format!("{revision}^{{commit}}");
    let output = git(
        root,
        &["rev-parse", "--verify", "--end-of-options", &expression],
    )?;
    if output.status.success() {
        return Ok(Some(String::from_utf8(output.stdout)?.trim().to_owned()));
    }
    if allow_unborn && revision == "HEAD" {
        let branch = git_text(root, &["symbolic-ref", "--quiet", "HEAD"]);
        if let Ok(branch) = branch {
            let exists = git(root, &["show-ref", "--verify", "--quiet", &branch])?;
            if !exists.status.success() {
                return Ok(None);
            }
        }
    }
    bail!("无法解析 Git 版本 {revision:?}，请使用已存在的提交、分支或标签")
}

fn validate_path(path: &str) -> Result<()> {
    if path.is_empty() || path.contains('\0') {
        bail!("文件路径不能为空或包含空字符");
    }
    let p = Path::new(path);
    if p.is_absolute()
        || !p
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
    {
        bail!("文件路径必须位于项目内：{path:?}");
    }
    if p.components()
        .any(|part| matches!(part, Component::Normal(name) if name == ".git" || name == ".reader"))
    {
        bail!("Git 内部文件和阅读元数据不属于源码对比范围");
    }
    Ok(())
}

fn excluded(path: &str) -> bool {
    Path::new(path)
        .components()
        .any(|part| matches!(part, Component::Normal(name) if name == ".git" || name == ".reader"))
}

fn tree(
    root: &Path,
    revision: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<BTreeMap<String, TreeEntry>> {
    let Some(revision) = revision else {
        return Ok(BTreeMap::new());
    };
    let bytes = git_ok(root, &["ls-tree", "-rz", "--full-tree", revision])?;
    let mut entries = BTreeMap::new();
    for item in bytes.split(|b| *b == 0).filter(|item| !item.is_empty()) {
        let Some(tab) = item.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let metadata = String::from_utf8_lossy(&item[..tab]);
        let mut fields = metadata.split_whitespace();
        let (Some(mode), Some(kind), Some(oid)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        let path = match std::str::from_utf8(&item[tab + 1..]) {
            Ok(path) => path,
            Err(_) => {
                warnings.push("跳过一个非 UTF-8 文件名，当前界面只支持 UTF-8 路径".into());
                continue;
            }
        };
        if excluded(path) {
            continue;
        }
        if let Err(error) = validate_path(path) {
            warnings.push(format!("跳过路径 {path:?}：{error}"));
            continue;
        }
        if kind != "blob" || (mode != "100644" && mode != "100755") {
            warnings.push(format!("跳过 {path}：子模块或符号链接不作为普通源码读取"));
            continue;
        }
        entries.insert(
            path.into(),
            TreeEntry {
                oid: oid.into(),
                mode: mode.into(),
            },
        );
    }
    Ok(entries)
}

fn file_names(root: &Path, args: &[&str], warnings: &mut Vec<String>) -> Result<BTreeSet<String>> {
    let bytes = git_ok(root, args)?;
    let mut names = BTreeSet::new();
    for item in bytes.split(|b| *b == 0).filter(|item| !item.is_empty()) {
        match std::str::from_utf8(item) {
            Ok(path) if !excluded(path) => {
                if validate_path(path).is_ok() {
                    names.insert(path.into());
                }
            }
            Ok(_) => {}
            Err(_) => warnings.push("跳过一个非 UTF-8 文件名".into()),
        }
    }
    Ok(names)
}

fn blob_bytes(root: &Path, entry: &TreeEntry) -> Result<Vec<u8>> {
    let size: u64 = git_text(root, &["cat-file", "-s", &entry.oid])?.parse()?;
    if size > MAX_FILE_BYTES {
        bail!("文件超过 2 MiB，已跳过文本比较");
    }
    let bytes = git_ok(root, &["cat-file", "blob", &entry.oid])?;
    Ok(bytes)
}

fn blob(root: &Path, entry: &TreeEntry) -> Result<String> {
    decode_text(blob_bytes(root, entry)?)
}

fn decode_text(bytes: Vec<u8>) -> Result<String> {
    if bytes.contains(&0) {
        bail!("二进制文件不提供文本对比");
    }
    String::from_utf8(bytes).context("非 UTF-8 文件不提供文本对比")
}

fn worktree_bytes(root: &Path, path: &str) -> Result<Option<(Vec<u8>, String)>> {
    validate_path(path)?;
    let mut current = root.to_path_buf();
    for component in Path::new(path).components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!("不读取符号链接：{path}");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error.into()),
        }
    }
    let metadata = fs::metadata(&current)?;
    if !metadata.is_file() {
        bail!("不是普通文件：{path}");
    }
    if metadata.len() > MAX_FILE_BYTES {
        bail!("文件超过 2 MiB，已跳过文本比较");
    }
    if !current.canonicalize()?.starts_with(root) {
        bail!("文件路径越过项目边界");
    }
    let mode = {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 != 0 {
                "100755"
            } else {
                "100644"
            }
        }
        #[cfg(not(unix))]
        {
            "100644"
        }
    };
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    fs::File::open(current)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        bail!("读取时文件超过 2 MiB，已跳过文本比较");
    }
    Ok(Some((bytes, mode.into())))
}

fn worktree_file(root: &Path, path: &str) -> Result<Option<(String, String)>> {
    worktree_bytes(root, path)?
        .map(|(bytes, mode)| Ok((decode_text(bytes)?, mode)))
        .transpose()
}

fn rename_hints(root: &Path, base: Option<&str>, head: Option<&str>) -> Vec<(String, String)> {
    let Some(base) = base else { return Vec::new() };
    let mut args = vec![
        "diff",
        "--name-status",
        "-z",
        "--find-renames",
        "--no-ext-diff",
        "--no-textconv",
    ];
    if head.is_none() {
        args.push("--cached");
    }
    args.push(base);
    if let Some(head) = head {
        args.push(head);
    }
    args.push("--");
    let Ok(bytes) = git_ok(root, &args) else {
        return Vec::new();
    };
    let parts: Vec<_> = bytes.split(|b| *b == 0).collect();
    let mut hints = Vec::new();
    let mut index = 0;
    while index < parts.len() {
        let status = parts[index];
        index += 1;
        if status.is_empty() {
            continue;
        }
        let count = if status[0] == b'R' || status[0] == b'C' {
            2
        } else {
            1
        };
        if index + count > parts.len() {
            break;
        }
        if status[0] == b'R' {
            if let (Ok(old), Ok(new)) = (
                std::str::from_utf8(parts[index]),
                std::str::from_utf8(parts[index + 1]),
            ) {
                hints.push((old.into(), new.into()));
            }
        }
        index += count;
    }
    hints
}

fn pair_renames(records: &mut Vec<Record>, hints: &[(String, String)]) {
    let mut pairs = Vec::new();
    let mut used = BTreeSet::new();
    for (old, new) in hints {
        let from = records
            .iter()
            .position(|r| r.path == *old && r.status == ChangeStatus::Deleted);
        let to = records.iter().position(|r| {
            r.path == *new && matches!(r.status, ChangeStatus::Added | ChangeStatus::Untracked)
        });
        if let (Some(from), Some(to)) = (from, to) {
            if !used.contains(&from) && !used.contains(&to) {
                pairs.push((from, to));
                used.insert(from);
                used.insert(to);
            }
        }
    }
    for (from, removed) in records.iter().enumerate() {
        if used.contains(&from) || removed.status != ChangeStatus::Deleted {
            continue;
        }
        let candidates: Vec<_> = records
            .iter()
            .enumerate()
            .filter(|(to, added)| {
                !used.contains(to)
                    && matches!(added.status, ChangeStatus::Added | ChangeStatus::Untracked)
                    && added.new == removed.old
            })
            .map(|(to, _)| to)
            .collect();
        if candidates.len() != 1 {
            continue;
        }
        let to = candidates[0];
        let reverse_count = records
            .iter()
            .enumerate()
            .filter(|(other, r)| {
                !used.contains(other)
                    && r.status == ChangeStatus::Deleted
                    && r.old == records[to].new
            })
            .count();
        if reverse_count == 1 {
            pairs.push((from, to));
            used.insert(from);
            used.insert(to);
        }
    }
    let merged: Vec<_> = pairs
        .iter()
        .map(|(from, to)| Record {
            path: records[*to].path.clone(),
            old_path: Some(records[*from].path.clone()),
            status: ChangeStatus::Renamed,
            old: records[*from].old.clone(),
            new: records[*to].new.clone(),
        })
        .collect();
    let mut index = 0;
    records.retain(|_| {
        let keep = !used.contains(&index);
        index += 1;
        keep
    });
    records.extend(merged);
    records.sort_by(|a, b| a.path.cmp(&b.path));
}

// Filesystems without executable permission bits must use the Git index mode.
// This also respects core.filemode=false on Unix (e.g. mounted Windows volumes).
fn index_modes(root: &Path) -> Result<BTreeMap<String, String>> {
    let bytes = git_ok(root, &["ls-files", "--stage", "-z"])?;
    let mut modes = BTreeMap::new();
    for item in bytes.split(|b| *b == 0).filter(|item| !item.is_empty()) {
        let Some(tab) = item.iter().position(|b| *b == b'\t') else {
            continue;
        };
        let metadata = String::from_utf8_lossy(&item[..tab]);
        let mut fields = metadata.split_whitespace();
        let (Some(mode), Some(_oid), Some(stage)) = (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if stage != "0" || !matches!(mode, "100644" | "100755") {
            continue;
        }
        let Ok(path) = std::str::from_utf8(&item[tab + 1..]) else {
            continue;
        };
        if !excluded(path) && validate_path(path).is_ok() {
            modes.insert(path.to_owned(), mode.to_owned());
        }
    }
    Ok(modes)
}

fn compare(root: &Path, base: &str, head: &str) -> Result<Comparison> {
    let base_oid = verify_commit(root, base, head == "WORKTREE")?;
    let head_oid = if head == "WORKTREE" {
        None
    } else {
        verify_commit(root, head, false)?
    };
    let mut warnings = Vec::new();
    if base_oid.is_none() {
        warnings.push("仓库还没有提交；以空快照对比当前工作区。".into());
    }
    let old_tree = tree(root, base_oid.as_deref(), &mut warnings)?;
    let new_tree = tree(root, head_oid.as_deref(), &mut warnings)?;
    let tracked = if head_oid.is_none() {
        file_names(root, &["ls-files", "--cached", "-z"], &mut warnings)?
    } else {
        BTreeSet::new()
    };
    let untracked = if head_oid.is_none() {
        file_names(
            root,
            &["ls-files", "--others", "--exclude-standard", "-z"],
            &mut warnings,
        )?
    } else {
        BTreeSet::new()
    };
    let mut names: BTreeSet<_> = old_tree.keys().chain(new_tree.keys()).cloned().collect();
    names.extend(tracked.iter().cloned());
    names.extend(untracked.iter().cloned());
    if names.len() > MAX_COMPARED_FILES {
        warnings.push(format!("项目超过 {MAX_COMPARED_FILES} 个文件，本次只比较按路径排序的前 {MAX_COMPARED_FILES} 个文件；其余未比较。"));
    }
    let use_index_modes = head_oid.is_none()
        && (cfg!(not(unix))
            || git_text(root, &["config", "--type=bool", "--get", "core.filemode"])
                .is_ok_and(|v| v == "false"));
    let modes = if use_index_modes {
        index_modes(root)?
    } else {
        BTreeMap::new()
    };
    let mut records = Vec::new();
    let mut compared_bytes = 0usize;
    for path in names.into_iter().take(MAX_COMPARED_FILES) {
        let old = match old_tree
            .get(&path)
            .map(|entry| blob_bytes(root, entry))
            .transpose()
        {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("跳过 {path}：{error}"));
                continue;
            }
        };
        let new = if head_oid.is_some() {
            new_tree
                .get(&path)
                .map(|entry| blob_bytes(root, entry).map(|text| (text, entry.mode.clone())))
                .transpose()
        } else if tracked.contains(&path) || untracked.contains(&path) {
            worktree_bytes(root, &path)
        } else {
            Ok(None)
        };
        let mut new = match new {
            Ok(value) => value,
            Err(error) => {
                warnings.push(format!("跳过 {path}：{error}"));
                continue;
            }
        };
        if use_index_modes {
            if let Some((_, mode)) = new.as_mut() {
                *mode = modes.get(&path).cloned().unwrap_or_else(|| "100644".into());
            }
        }
        compared_bytes = compared_bytes
            .saturating_add(old.as_ref().map_or(0, Vec::len))
            .saturating_add(new.as_ref().map_or(0, |(text, _)| text.len()));
        if compared_bytes > MAX_COMPARED_BYTES {
            warnings.push(
                "前后版本文本合计达到 64 MiB 对比上限；其余文件未比较，请选择更小的工程。".into(),
            );
            break;
        }
        let old_mode = old_tree.get(&path).map(|entry| entry.mode.as_str());
        // Compare bytes and executable mode before text decoding: unchanged assets are harmless.
        if let (Some(old_bytes), Some((new_bytes, mode))) = (&old, &new) {
            if old_bytes == new_bytes && old_mode == Some(mode.as_str()) {
                continue;
            }
        }
        let decoded = (|| -> Result<_> {
            Ok((
                old.map(decode_text).transpose()?,
                new.map(|(bytes, mode)| Ok::<_, anyhow::Error>((decode_text(bytes)?, mode)))
                    .transpose()?,
            ))
        })();
        let (old, new) = match decoded {
            Ok(pair) => pair,
            Err(error) => {
                warnings.push(format!("跳过 {path}：{error}"));
                continue;
            }
        };
        let record = match (old, new) {
            (None, None) => continue,
            (Some(old), Some((new, mode))) if old == new && old_mode == Some(mode.as_str()) => {
                continue
            }
            (Some(old), Some((new, _))) => Record {
                path,
                old_path: None,
                status: ChangeStatus::Modified,
                old,
                new,
            },
            (Some(old), None) => Record {
                path,
                old_path: None,
                status: ChangeStatus::Deleted,
                old,
                new: String::new(),
            },
            (None, Some((new, _))) => {
                let status = if head_oid.is_none() && !tracked.contains(&path) {
                    ChangeStatus::Untracked
                } else {
                    ChangeStatus::Added
                };
                Record {
                    path,
                    old_path: None,
                    status,
                    old: String::new(),
                    new,
                }
            }
        };
        records.push(record);
    }
    pair_renames(
        &mut records,
        &rename_hints(root, base_oid.as_deref(), head_oid.as_deref()),
    );
    Ok(Comparison {
        records,
        warnings,
        base_oid,
        head_oid,
    })
}

fn make_hunks(old: &str, new: &str) -> Vec<DiffHunk> {
    let diff = TextDiff::from_lines(old, new);
    diff.grouped_ops(3)
        .iter()
        .filter_map(|group| {
            let first = group.first()?;
            let last = group.last()?;
            let old_len = last.old_range().end - first.old_range().start;
            let new_len = last.new_range().end - first.new_range().start;
            let old_start = first.old_range().start + usize::from(old_len > 0);
            let new_start = first.new_range().start + usize::from(new_len > 0);
            let mut lines = Vec::new();
            for op in group {
                for change in diff.iter_changes(op) {
                    lines.push(DiffLine {
                        kind: match change.tag() {
                            ChangeTag::Delete => DiffLineKind::Remove,
                            ChangeTag::Insert => DiffLineKind::Add,
                            ChangeTag::Equal => DiffLineKind::Context,
                        },
                        text: change.value().trim_end_matches('\n').to_string(),
                        old_line: change.old_index().map(|line| (line + 1) as u32),
                        new_line: change.new_index().map(|line| (line + 1) as u32),
                    });
                }
            }
            Some(DiffHunk {
                old_start: old_start as u32,
                old_lines: old_len as u32,
                new_start: new_start as u32,
                new_lines: new_len as u32,
                header: format!("@@ -{old_start},{old_len} +{new_start},{new_len} @@"),
                lines,
            })
        })
        .collect()
}

fn overlaps(range: &CodeRange, lines: &BTreeSet<u32>) -> bool {
    lines
        .range(range.start_line..=range.end_line)
        .next()
        .is_some()
}

/// Compare a commit (or HEAD) with the actual worktree or another commit.
pub fn changes(
    root: &Path,
    base: &str,
    head: &str,
    snapshot: &ProjectSnapshot,
) -> Result<ChangeSummary> {
    let mut summary = ChangeSummary {
        base: base.into(),
        head: head.into(),
        files: Vec::new(),
        warnings: Vec::new(),
    };
    if !info(root).available {
        summary
            .warnings
            .push("当前目录不是 Git 工作区（或 Git 不可用）；源码阅读和功能标注仍可使用。".into());
        return Ok(summary);
    }
    let root = match comparison_root(root) {
        Ok(root) => root,
        Err(error) => {
            summary.warnings.push(error.to_string());
            return Ok(summary);
        }
    };
    let comparison = compare(&root, base, head)?;
    summary.warnings = comparison.warnings;
    if comparison.head_oid.is_some() {
        summary
            .warnings
            .push("提交间对比使用历史源码；功能关联参考当前功能索引，旧版本关系请核对。".into());
    }
    for record in comparison.records {
        let hunks = make_hunks(&record.old, &record.new);
        let old_lines: BTreeSet<_> = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|line| line.kind == DiffLineKind::Remove)
            .filter_map(|line| line.old_line)
            .collect();
        let new_lines: BTreeSet<_> = hunks
            .iter()
            .flat_map(|h| &h.lines)
            .filter(|line| line.kind == DiffLineKind::Add)
            .filter_map(|line| line.new_line)
            .collect();
        let old_path = record.old_path.as_deref().unwrap_or(&record.path);
        let mut parse_evidence =
            |path: &str, content: &str, side: &str| match crate::parse::parse_with_status(
                path, content,
            ) {
                Ok((symbols, has_errors)) => {
                    if has_errors {
                        summary.warnings.push(format!(
                            "{path}：{side}版本包含语法错误，符号影响面可能不完整。"
                        ));
                    }
                    symbols
                }
                Err(error) => {
                    summary.warnings.push(format!(
                        "{path}：无法解析{side}版本的符号（{error}），仍提供文本差异。"
                    ));
                    Vec::new()
                }
            };
        let old_symbols = parse_evidence(old_path, &record.old, "旧");
        let new_symbols = parse_evidence(&record.path, &record.new, "新");
        let mut affected = BTreeSet::new();
        for symbol in &old_symbols {
            if overlaps(&symbol.range, &old_lines) || record.status == ChangeStatus::Renamed {
                affected.insert(symbol.id.clone());
            }
        }
        for symbol in &new_symbols {
            if overlaps(&symbol.range, &new_lines) || record.status == ChangeStatus::Renamed {
                affected.insert(symbol.id.clone());
            }
        }
        let mut features = BTreeSet::new();
        let mut approximate = false;
        for feature in &snapshot.features {
            let hit = feature.anchors.iter().any(|anchor| {
                if anchor
                    .symbol_id
                    .as_ref()
                    .is_some_and(|id| affected.contains(id))
                {
                    return true;
                }
                if anchor.range.file == record.path && overlaps(&anchor.range, &new_lines) {
                    return true;
                }
                if matches!(record.status, ChangeStatus::Deleted | ChangeStatus::Renamed)
                    && (anchor.range.file == old_path || anchor.range.file == record.path)
                {
                    return true;
                }
                if anchor.kind == AnchorKind::Range
                    && anchor.range.file == old_path
                    && overlaps(&anchor.range, &old_lines)
                {
                    approximate = true;
                    return true;
                }
                false
            });
            if hit {
                features.insert(feature.id.clone());
            }
        }
        if approximate {
            summary.warnings.push(format!(
                "{}：删除行与范围锚点的关联按已知行范围估计，请结合前后版本核对。",
                record.path
            ));
        }
        summary.files.push(ChangedFile {
            path: record.path,
            old_path: record.old_path,
            status: record.status,
            additions: new_lines.len() as u32,
            deletions: old_lines.len() as u32,
            hunks,
            affected_symbols: affected.into_iter().collect(),
            feature_ids: features.into_iter().collect(),
        });
    }
    Ok(summary)
}

/// Return exact before/after text, including deleted files and rename origins.
pub fn file_diff(root: &Path, path: &str, base: &str, head: &str) -> Result<FileDiff> {
    validate_path(path)?;
    let root = comparison_root(root)?;
    let comparison = compare(&root, base, head)?;
    if let Some(record) = comparison
        .records
        .into_iter()
        .find(|record| record.path == path || record.old_path.as_deref() == Some(path))
    {
        return Ok(FileDiff {
            language: crate::parse::language_for(&record.path).into(),
            path: record.path,
            old_path: record.old_path,
            old_content: record.old,
            new_content: record.new,
        });
    }
    // Opening an unchanged file in diff mode is still useful. Read both sides
    // directly rather than reporting an empty change as an error.
    let mut warnings = Vec::new();
    let old_tree = tree(&root, comparison.base_oid.as_deref(), &mut warnings)?;
    let new_tree = tree(&root, comparison.head_oid.as_deref(), &mut warnings)?;
    let old_content = old_tree
        .get(path)
        .map(|entry| blob(&root, entry))
        .transpose()?;
    let new_content = if comparison.head_oid.is_some() {
        new_tree
            .get(path)
            .map(|entry| blob(&root, entry))
            .transpose()?
    } else {
        worktree_file(&root, path)?.map(|(text, _)| text)
    };
    if old_content.is_none() && new_content.is_none() {
        return Err(anyhow!("该文件在所选版本中不存在：{path}"));
    }
    Ok(FileDiff {
        path: path.into(),
        old_path: None,
        old_content: old_content.unwrap_or_default(),
        new_content: new_content.unwrap_or_default(),
        language: crate::parse::language_for(path).into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn run(root: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Reader Test")
            .env("GIT_AUTHOR_EMAIL", "reader@example.invalid")
            .env("GIT_COMMITTER_NAME", "Reader Test")
            .env("GIT_COMMITTER_EMAIL", "reader@example.invalid")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn repo() -> TempDir {
        let temp = TempDir::new().unwrap();
        run(temp.path(), &["init", "-q"]);
        temp
    }

    fn commit(root: &Path) {
        run(root, &["add", "--all"]);
        run(root, &["commit", "-qm", "fixture"]);
    }

    fn snapshot(root: &Path) -> ProjectSnapshot {
        ProjectSnapshot {
            overview: None,
            root: root.display().to_string(),
            name: "fixture".into(),
            git: info(root),
            files: Vec::new(),
            symbols: Vec::new(),
            features: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn index_modes_preserve_executable_files_when_filemode_is_disabled() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("start.command"), "echo hello\n").unwrap();
        commit(root);
        run(root, &["config", "core.filemode", "false"]);
        run(root, &["update-index", "--chmod=+x", "start.command"]);
        run(root, &["commit", "-qm", "record executable index mode"]);
        assert!(changes(root, "HEAD", "WORKTREE", &snapshot(root))
            .unwrap()
            .files
            .is_empty());
        run(root, &["update-index", "--chmod=-x", "start.command"]);
        let diff = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "start.command");
    }

    #[test]
    fn final_worktree_combines_staged_unstaged_and_untracked() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("logic.py"), "def run():\n    return 1\n").unwrap();
        fs::write(root.join("deleted.py"), "gone = True\n").unwrap();
        commit(root);
        fs::write(root.join("logic.py"), "def run():\n    return 2\n").unwrap();
        run(root, &["add", "logic.py"]);
        fs::write(root.join("logic.py"), "def run():\n    return 3\n").unwrap();
        fs::remove_file(root.join("deleted.py")).unwrap();
        fs::write(root.join("new file.py"), "fresh = 1\n").unwrap();
        fs::create_dir(root.join(".reader")).unwrap();
        fs::write(root.join(".reader/private.json"), "{}").unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert_eq!(result.files.len(), 3);
        let changed = result
            .files
            .iter()
            .find(|file| file.path == "logic.py")
            .unwrap();
        assert_eq!(changed.additions, 1);
        assert_eq!(changed.deletions, 1);
        assert!(changed
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|line| line.text.contains("return 3")));
        assert!(!changed
            .hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|line| line.text.contains("return 2")));
        assert_eq!(
            result
                .files
                .iter()
                .find(|f| f.path == "new file.py")
                .unwrap()
                .status,
            ChangeStatus::Untracked
        );
        let deleted = file_diff(root, "deleted.py", "HEAD", "WORKTREE").unwrap();
        assert_eq!(deleted.old_content, "gone = True\n");
        assert!(deleted.new_content.is_empty());
        assert_eq!(
            fs::read_to_string(root.join("logic.py")).unwrap(),
            "def run():\n    return 3\n"
        );
    }

    #[test]
    fn staged_rename_with_edits_preserves_old_text() {
        let repo = repo();
        let root = repo.path();
        fs::write(
            root.join("old name.py"),
            "def work():\n    alpha = 1\n    beta = 2\n    return alpha + beta\n",
        )
        .unwrap();
        commit(root);
        run(root, &["mv", "old name.py", "新 name.py"]);
        fs::write(
            root.join("新 name.py"),
            "def work():\n    alpha = 1\n    beta = 3\n    return alpha + beta\n",
        )
        .unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].status, ChangeStatus::Renamed);
        assert_eq!(result.files[0].old_path.as_deref(), Some("old name.py"));
        let diff = file_diff(root, "新 name.py", "HEAD", "WORKTREE").unwrap();
        assert!(diff.old_content.contains("beta = 2"));
        assert!(diff.new_content.contains("beta = 3"));
    }

    #[test]
    fn unborn_and_non_git_directories_are_supported() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("first.rs"), "fn main() {}\n").unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert_eq!(result.files.len(), 1);
        assert!(!result.warnings.is_empty());
        let ordinary = TempDir::new().unwrap();
        assert!(!info(ordinary.path()).available);
        let result = changes(
            ordinary.path(),
            "HEAD",
            "WORKTREE",
            &snapshot(ordinary.path()),
        )
        .unwrap();
        assert!(result.files.is_empty());
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn commit_comparison_is_independent_of_worktree() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("a.py"), "value = 1\n").unwrap();
        commit(root);
        let old = info(root).head.unwrap();
        fs::write(root.join("a.py"), "value = 2\n").unwrap();
        commit(root);
        fs::write(root.join("a.py"), "value = 999\n").unwrap();
        let result = file_diff(root, "a.py", &old, "HEAD").unwrap();
        assert_eq!(result.old_content, "value = 1\n");
        assert_eq!(result.new_content, "value = 2\n");
    }

    #[test]
    fn line_impacts_exclude_nearby_unchanged_functions() {
        let repo = repo();
        let root = repo.path();
        let old = "def first():\n    return 1\n\ndef second():\n    return 2\n";
        let new = "def first():\n    return 9\n\ndef second():\n    return 2\n";
        fs::write(root.join("a.py"), old).unwrap();
        commit(root);
        fs::write(root.join("a.py"), new).unwrap();
        let symbols = crate::parse::parse_symbols("a.py", new).unwrap();
        let first = symbols.iter().find(|s| s.name == "first").unwrap();
        let second = symbols.iter().find(|s| s.name == "second").unwrap();
        let mut project = snapshot(root);
        project.features = [first, second]
            .iter()
            .enumerate()
            .map(|(index, symbol)| Feature {
                id: format!("f{index}"),
                name: symbol.name.clone(),
                description: String::new(),
                color: "#aabbcc".into(),
                parent_id: None,
                review_state: ReviewState::Unread,
                reviewed_at: None,
                reviewed_digest: None,
                current_digest: String::new(),
                agent_delivery: None,
                anchors: vec![AnchorBinding {
                    id: format!("a{index}"),
                    kind: AnchorKind::Symbol,
                    symbol_id: Some(symbol.id.clone()),
                    symbol_name: Some(symbol.name.clone()),
                    range: symbol.range.clone(),
                    content_hash: String::new(),
                    status: AnchorStatus::Exact,
                    note: String::new(),
                    role: None,
                    responsibility: String::new(),
                }],
            })
            .collect();
        let mut shared_feature = project.features[0].clone();
        shared_feature.id = "shared".into();
        shared_feature.name = "Another feature using first".into();
        project.features.push(shared_feature);
        let result = changes(root, "HEAD", "WORKTREE", &project).unwrap();
        assert!(result.files[0].affected_symbols.contains(&first.id));
        assert!(!result.files[0].affected_symbols.contains(&second.id));
        assert_eq!(result.files[0].feature_ids, vec!["f0", "shared"]);
    }

    #[test]
    fn pure_deletion_uses_old_symbols_without_blaming_next_function() {
        let repo = repo();
        let root = repo.path();
        let old = "def removed():\n    return 1\n\ndef retained():\n    return 2\n";
        let new = "def retained():\n    return 2\n";
        fs::write(root.join("a.py"), old).unwrap();
        commit(root);
        fs::write(root.join("a.py"), new).unwrap();
        let symbols = crate::parse::parse_symbols("a.py", old).unwrap();
        let removed = symbols.iter().find(|s| s.name == "removed").unwrap();
        let retained = symbols.iter().find(|s| s.name == "retained").unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert!(result.files[0].affected_symbols.contains(&removed.id));
        assert!(!result.files[0].affected_symbols.contains(&retained.id));
        assert_eq!(result.files[0].additions, 0);
    }

    #[test]
    fn rejects_path_and_revision_injection_and_subdirectory_mismatch() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("a.py"), "a = 1\n").unwrap();
        commit(root);
        assert!(file_diff(root, "../a.py", "HEAD", "WORKTREE").is_err());
        assert!(file_diff(root, "/etc/passwd", "HEAD", "WORKTREE").is_err());
        assert!(file_diff(root, ".git/config", "HEAD", "WORKTREE").is_err());
        assert!(changes(root, "--help", "WORKTREE", &snapshot(root)).is_err());
        fs::create_dir(root.join("sub")).unwrap();
        let sub = changes(&root.join("sub"), "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert!(sub.files.is_empty());
        assert!(sub.warnings.iter().any(|w| w.contains("仓库根目录")));
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_worktree_symlinks() {
        use std::os::unix::fs::symlink;
        let repo = repo();
        let root = repo.path();
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret"), "do not read").unwrap();
        symlink(outside.path().join("secret"), root.join("link.py")).unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert!(result.files.is_empty());
        assert!(result.warnings.iter().any(|w| w.contains("符号链接")));
        assert!(file_diff(root, "link.py", "HEAD", "WORKTREE").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn worktree_comparison_never_executes_clean_filters() {
        let repo = repo();
        let root = repo.path();
        fs::write(root.join("a.py"), "a = 1\n").unwrap();
        commit(root);
        fs::write(root.join(".gitattributes"), "*.py filter=tripwire\n").unwrap();
        run(
            root,
            &[
                "config",
                "filter.tripwire.clean",
                "touch SHOULD_NOT_EXIST; cat",
            ],
        );
        fs::write(root.join("a.py"), "a = 2\n").unwrap();
        let result = changes(root, "HEAD", "WORKTREE", &snapshot(root)).unwrap();
        assert!(result.files.iter().any(|f| f.path == "a.py"));
        assert!(!root.join("SHOULD_NOT_EXIST").exists());
    }
}
