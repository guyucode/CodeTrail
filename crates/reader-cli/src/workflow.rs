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
    #[serde(default)]
    record: Option<ValidationRecord>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ValidationRecord {
    goal: String,
    path: String,
    start_line: usize,
    end_line: usize,
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
    ensure!(
        plan.checks.iter().all(|c| !c.program.trim().is_empty()),
        "检查程序不能为空"
    );
    ensure!(
        plan.record.is_none() || plan.checks.len() <= 200,
        "验证记录最多200条检查"
    );
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
    let recording = plan
        .record
        .as_ref()
        .map(|r| begin_record(root, r))
        .transpose()?;
    let mut recording_errors = Vec::new();
    let mut checks = Vec::new();
    for (index, c) in plan.checks.iter().enumerate() {
        ensure!(!c.program.trim().is_empty(), "检查程序不能为空");
        let started = std::time::Instant::now();
        let result = Command::new(&c.program)
            .args(&c.args)
            .current_dir(root)
            .output();
        match result {
            Ok(out) => checks.push(json!({"program":c.program,"args":c.args,"ok":out.status.success(),
                "exitCode":out.status.code(),"stdout":String::from_utf8_lossy(&out.stdout),"stderr":String::from_utf8_lossy(&out.stderr)})),
            Err(e) => checks.push(json!({"program":c.program,"args":c.args,"ok":false,"error":e.to_string()}))
        }
        checks.last_mut().unwrap()["elapsedMs"] = json!(started.elapsed().as_millis() as u64);
        if let Some((id, source)) = &recording {
            if let Err(error) = record_check(root, id, source, index, checks.last().unwrap()) {
                recording_errors.push(error.to_string());
            }
        }
    }
    let after_result = coverage(root, &plan.base);
    let inventory_after = agent::inspect(root);
    let final_associations = association_check(root);
    let mut post_check_errors = Vec::new();
    let stable = after_result.as_ref().is_ok_and(|after| &before == after)
        && inventory_after
            .as_ref()
            .is_ok_and(|after| &inventory_before == after);
    let associations_ok = final_associations.as_ref().is_ok_and(|ok| *ok);
    if let Err(e) = &after_result {
        post_check_errors.push(e.to_string());
    }
    if let Err(e) = &inventory_after {
        post_check_errors.push(e.to_string());
    }
    if let Err(e) = &final_associations {
        post_check_errors.push(e.to_string());
    }
    let after = after_result.unwrap_or(Value::Null);
    let checks_ok = stable && associations_ok && checks.iter().all(|c| c["ok"] == true);
    if let Some((id, _)) = &recording {
        if let Err(error) = finish_record(
            root,
            id,
            plan.record.as_ref().unwrap(),
            &checks,
            stable,
            checks_ok && recording_errors.is_empty(),
        ) {
            recording_errors.push(error.to_string());
        }
    }
    let ok = checks_ok && recording_errors.is_empty();
    Ok(
        json!({"ok":ok,"associationsOk":associations_ok,"coverage":after,"postCheckErrors":post_check_errors,
        "sourceAndMetadataStable":stable,"checks":checks,
        "verificationRunId":recording.as_ref().map(|(id,_)| id),"verificationErrors":recording_errors,
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

fn begin_record(
    root: &Path,
    config: &ValidationRecord,
) -> Result<(String, reader_core::trace::RuntimeLocation)> {
    use reader_core::trace;
    use std::sync::atomic::{AtomicU64, Ordering};
    static SERIAL: AtomicU64 = AtomicU64::new(0);
    ensure!(
        !config.goal.trim().is_empty() && config.goal.chars().count() <= 2000,
        "验证目标为空或过长"
    );
    let source = trace::capture_location(root, &config.path, config.start_line, config.end_line)?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    let id = format!(
        "validation-{}-{stamp}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    );
    trace::start(
        root,
        trace::RunStart {
            id: id.clone(),
            label: config.goal.chars().take(100).collect(),
            kind: "isolatedTest".into(),
            producer: "reader-cli agent validate".into(),
            summary: "实际执行调用者选定的交付检查；代码位置代表验证目标，不推断完整业务调用路径。"
                .into(),
        },
    )?;
    Ok((id, source))
}

fn record_check(
    root: &Path,
    id: &str,
    source: &reader_core::trace::RuntimeLocation,
    index: usize,
    check: &Value,
) -> Result<()> {
    use reader_core::trace::{self, ObservedValue, RuntimeStep, StepRequest};
    let program = Path::new(check["program"].as_str().unwrap_or("检查程序"))
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    let exit = check["exitCode"]
        .as_i64()
        .map(|n| n.to_string())
        .unwrap_or_else(|| "未正常退出或未启动".into());
    trace::append(
        root,
        StepRequest {
            run_id: id.into(),
            step: RuntimeStep {
                id: format!("check-{}", index + 1),
                title: format!("检查{}：{program}", index + 1),
                status: if check["ok"] == true { "ok" } else { "error" }.into(),
                result: format!("实际退出码：{exit}。"),
                reason: "实际执行显式检查命令；此处位置仅指验证目标，不证明该函数被调用。".into(),
                inputs: vec![ObservedValue {
                    name: "程序".into(),
                    value: program.into_owned(),
                }],
                outputs: vec![ObservedValue {
                    name: "退出码".into(),
                    value: exit,
                }],
                location: source.clone(),
                elapsed_ms: check["elapsedMs"].as_u64().unwrap_or(0),
            },
        },
    )?;
    Ok(())
}

fn finish_record(
    root: &Path,
    id: &str,
    config: &ValidationRecord,
    checks: &[Value],
    stable: bool,
    ok: bool,
) -> Result<()> {
    use reader_core::trace::{self, ObservedValue, RunFinish, Verification};
    let passed = checks.iter().filter(|c| c["ok"] == true).count();
    let mut actual = format!(
        "执行{}条检查：成功{}，失败{}；{}。",
        checks.len(),
        passed,
        checks.len() - passed,
        if stable {
            "源码与功能关联保持一致"
        } else {
            "源码或功能关联发生变化"
        }
    );
    if !ok && passed == checks.len() && stable {
        actual.push_str("记录或交付校验未完整通过，不能确认验证成功。");
    }
    let value = |name: &str, value: String| ObservedValue {
        name: name.into(),
        value,
    };
    trace::finish(
        root,
        RunFinish {
            run_id: id.into(),
            status: if ok { "completed" } else { "failed" }.into(),
            summary: "实际执行选定的交付检查；不等同于业务验收，不包含未采集的业务中间值。".into(),
            verification: Some(Verification {
                goal: config.goal.clone(),
                inputs: vec![value("检查命令数", checks.len().to_string())],
                expected: "选定的检查命令全部成功，源码和功能关联在检查期间保持一致。".into(),
                actual,
                key_values: vec![
                    value("成功检查数", passed.to_string()),
                    value("失败检查数", (checks.len() - passed).to_string()),
                    value("源码与关联一致", stable.to_string()),
                ],
                verdict: if ok { "matched" } else { "different" }.into(),
            }),
        },
    )?;
    Ok(())
}
