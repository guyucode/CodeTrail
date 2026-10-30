//! Agent authoring helpers and explicit, reviewable delivery gates.
use anyhow::{bail, ensure, Context, Result};
use reader_core::{agent, git, open_project, types::*};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path, process::Command};

pub fn draft(root: &Path, ids: &[String]) -> Result<Value> {
    let inventory = agent::inspect(root)?;
    let project = open_project(root)?;
    let mut found = BTreeSet::new();
    let mut features = Vec::new();
    for f in &project.features {
        if (ids.is_empty() && f.agent_delivery.is_some()) || ids.contains(&f.id) {
            found.insert(f.id.clone());
            let evidence: Vec<_> = f
                .anchors
                .iter()
                .map(|a| {
                    json!({
                        "id":a.id,"path":a.range.file,"symbolId":a.symbol_id,
                        "startLine":a.range.start_line,"endLine":a.range.end_line,
                        "expectedFileHash":inventory["fileHashes"][&a.range.file],
                        "role":a.role,"responsibility":a.responsibility
                    })
                })
                .collect();
            features.push(json!({"id":f.id,"expectedDigest":f.current_digest,"name":f.name,
                "description":f.description,"summary":"TODO: 核对说明、职责和范围后填写本次变更摘要",
                "evidence":evidence}));
        }
    }
    ensure!(ids.iter().all(|id| found.contains(id)), "存在未知功能 ID");
    ensure!(
        !features.is_empty(),
        "没有可生成草稿的功能；新增功能按协议提供稳定 ID 与真实证据"
    );
    Ok(
        json!({"schemaVersion":1,"deliveryId":"draft-replace-before-delivery",
        "expectedRevision":inventory["revision"],"overview":inventory["project"].get("overview"),"features":features}),
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Plan {
    schema_version: u32,
    base: String,
    #[serde(default)]
    unassociated: Vec<Exception>,
    checks: Vec<Check>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Exception {
    path: String,
    change_digest: String,
    reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Check {
    program: String,
    args: Vec<String>,
}

pub fn coverage(root: &Path, base: &str) -> Result<Value> {
    let project = open_project(root)?;
    let summary = git::changes(root, base, "WORKTREE", &project)?;
    ensure!(
        summary.warnings.is_empty(),
        "差异不完整：{:?}",
        summary.warnings
    );
    let mut files = Vec::new();
    for f in summary.files {
        // Generated delivery and human records are checked separately, never source coverage.
        if f.path.starts_with(".reader/") {
            continue;
        }
        let mut uncovered = Vec::new();
        for line in f.hunks.iter().flat_map(|h| &h.lines) {
            if line.kind != DiffLineKind::Add || line.text.trim().is_empty() {
                continue;
            }
            if let Some(n) = line.new_line {
                let covered = project.features.iter().flat_map(|f| &f.anchors).any(|a| {
                    a.range.file == f.path
                        && matches!(a.status, AnchorStatus::Exact | AnchorStatus::Moved)
                        && a.range.start_line <= n
                        && n <= a.range.end_line
                });
                if !covered {
                    uncovered.push(n);
                }
            }
        }
        let digest = format!("{:x}", Sha256::digest(serde_json::to_vec(&f)?));
        // Deletion-side meaning cannot be inferred from current anchors. Require an explanation.
        let needs_explanation = !uncovered.is_empty() || f.deletions > 0 || f.hunks.is_empty();
        files.push(
            json!({"path":f.path,"oldPath":f.old_path,"featureIds":f.feature_ids,
            "uncoveredLines":uncovered,"deletedLines":f.deletions,"changeDigest":digest,
            "needsExplanation":needs_explanation}),
        );
    }
    Ok(
        json!({"base":base,"files":files,"note":"逐行检查新增内容；删除、纯重命名或无法表达的差异需说明。关联不证明语义覆盖。"}),
    )
}

fn association_check(root: &Path) -> Result<bool> {
    let p = open_project(root)?;
    Ok(agent::blocking_warnings(&p.warnings).is_empty()
        && p.features
            .iter()
            .filter_map(|f| f.agent_delivery.as_ref())
            .all(|d| d.matches_source))
}

pub fn validate(root: &Path, bytes: &[u8]) -> Result<Value> {
    let plan: Plan = serde_json::from_slice(bytes).context("无效的交付检查计划")?;
    ensure!(plan.schema_version == 1, "不支持的检查计划版本");
    ensure!(!plan.checks.is_empty(), "必须明确列出实际执行的检查命令");
    let before = coverage(root, &plan.base)?;
    let files = before["files"].as_array().unwrap();
    let mut used = BTreeSet::new();
    let mut missing = Vec::new();
    for f in files.iter().filter(|f| f["needsExplanation"] == true) {
        let matches: Vec<_> = plan
            .unassociated
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                f["path"] == e.path
                    && f["changeDigest"] == e.change_digest
                    && !e.reason.trim().is_empty()
            })
            .collect();
        if matches.len() == 1 {
            used.insert(matches[0].0);
        } else {
            missing.push(f.clone());
        }
    }
    let associations_ok = association_check(root)?;
    let plan_ok = missing.is_empty() && used.len() == plan.unassociated.len();
    if !associations_ok || !plan_ok {
        return Ok(
            json!({"ok":false,"associationsOk":associations_ok,"coverage":before,
            "unexplained":missing,"planOk":plan_ok,"checks":[],"error":"先修复关联或未说明差异；多余、重复、过期理由同样拒绝"}),
        );
    }
    let inventory_before = agent::inspect(root)?;
    let mut checks = Vec::new();
    for c in &plan.checks {
        ensure!(!c.program.trim().is_empty(), "检查程序不能为空");
        let result = Command::new(&c.program)
            .args(&c.args)
            .current_dir(root)
            .output();
        match result {
            Ok(out) => checks.push(json!({"program":c.program,"args":c.args,"ok":out.status.success(),
                "exitCode":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)})),
            Err(e) => checks.push(json!({"program":c.program,"args":c.args,"ok":false,"error":e.to_string()}))
        }
    }
    let after = coverage(root, &plan.base)?;
    let inventory_after = agent::inspect(root)?;
    let stable = before == after && inventory_before == inventory_after;
    let ok = stable && association_check(root)? && checks.iter().all(|c| c["ok"] == true);
    Ok(
        json!({"ok":ok,"associationsOk":association_check(root)?,"coverage":after,
        "sourceAndMetadataStable":stable,"checks":checks,
        "note":"执行的是调用者明确提供的命令；不推断测试充分性，不产生人工确认。"}),
    )
}

pub fn read_input() -> Result<Vec<u8>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(2 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 2 * 1024 * 1024 {
        bail!("JSON 超出 2 MiB");
    }
    Ok(bytes)
}
