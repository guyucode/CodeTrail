mod workflow;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    path::Path,
};

fn field<'a>(payload: &'a Value, key: &str) -> Result<&'a str> {
    payload
        .get(key)
        .and_then(Value::as_str)
        .with_context(|| format!("missing string field: {key}"))
}

fn dispatch(root: &Path, request: Value) -> Result<Value> {
    let command = field(&request, "command")?;
    let payload = request.get("payload").cloned().unwrap_or_else(|| json!({}));
    let value = match command {
        "openProject" | "refresh" => serde_json::to_value(reader_core::open_project(root)?)?,
        "readFile" => {
            serde_json::to_value(reader_core::read_file(root, field(&payload, "path")?)?)?
        }
        "saveFeature" => serde_json::to_value(reader_core::save_feature(
            root,
            serde_json::from_value(payload)?,
        )?)?,
        "deleteFeature" => serde_json::to_value(reader_core::delete_feature(
            root,
            field(&payload, "featureId")?,
        )?)?,
        "bindAnchor" => serde_json::to_value(reader_core::bind_anchor(
            root,
            serde_json::from_value(payload)?,
        )?)?,
        "removeAnchor" => serde_json::to_value(reader_core::remove_anchor(
            root,
            field(&payload, "featureId")?,
            field(&payload, "anchorId")?,
        )?)?,
        "setReview" => serde_json::to_value(reader_core::set_review(
            root,
            field(&payload, "featureId")?,
            serde_json::from_value(payload.get("state").cloned().context("missing state")?)?,
            payload.get("expectedDigest").and_then(Value::as_str),
        )?)?,
        "getChanges" => {
            let snapshot = reader_core::open_project(root)?;
            serde_json::to_value(reader_core::git::changes(
                root,
                field(&payload, "base")?,
                field(&payload, "head")?,
                &snapshot,
            )?)?
        }
        "getFileDiff" => serde_json::to_value(reader_core::git::file_diff(
            root,
            field(&payload, "path")?,
            field(&payload, "base")?,
            field(&payload, "head")?,
        )?)?,
        "getRelations" => serde_json::to_value(reader_core::relations::analyze(
            root,
            field(&payload, "symbolId")?,
            payload
                .get("base")
                .and_then(Value::as_str)
                .unwrap_or("HEAD"),
            payload
                .get("head")
                .and_then(Value::as_str)
                .unwrap_or("WORKTREE"),
            payload
                .get("semantic")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        )?)?,
        "exportReview" => Value::String(reader_core::export_review(root)?),
        _ => bail!("unknown command: {command}"),
    };
    Ok(value)
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.get(1).is_some_and(|s| s == "trace") {
        return run_trace(&args);
    }
    if args.get(1).is_some_and(|s| s == "agent") {
        return run_agent(&args);
    }
    if args.len() != 3 || !matches!(args[1].as_str(), "verify" | "rpc") {
        bail!("Usage: reader-cli verify <repository>\n       reader-cli rpc <repository> < request.json");
    }
    let root = Path::new(&args[2]);
    if args[1] == "verify" {
        let snapshot = reader_core::open_project(root)?;
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "name": snapshot.name, "root": snapshot.root, "files": snapshot.files.len(),
                "symbols": snapshot.symbols.len(), "features": snapshot.features.len(),
                "git": snapshot.git, "warnings": snapshot.warnings,
                "note": "Syntax definitions only; this verification does not execute project code or claim call-graph completeness."
            }))?
        );
    } else {
        // Read one bounded JSON frame, rather than waiting for the caller to
        // close a bidirectional pipe before dispatching a complete object.
        let input = io::stdin().take(1024 * 1024);
        let request = serde_json::Deserializer::from_reader(input)
            .into_iter::<Value>()
            .next()
            .context("request must contain a JSON object")?
            .context("request must be JSON")?;
        let result = dispatch(root, request)?;
        println!("{}", json!({"ok": true, "value": result}));
    }
    Ok(())
}

fn run_agent(args: &[String]) -> Result<()> {
    if args.len() < 4 {
        bail!("Usage: reader-cli agent <inspect|deliver|check|draft|coverage|validate|report> <repository>; deliver reads JSON from stdin");
    }
    let root = Path::new(&args[3]);
    let action = args[2].as_str();
    match action {
        "draft" => {
            println!("{}", workflow::draft(root, &args[4..])?);
            return Ok(());
        }
        "coverage" if args.len() == 5 => {
            println!("{}", workflow::coverage(root, &args[4])?);
            return Ok(());
        }
        "validate" if args.len() == 4 => {
            let report = workflow::validate(root, &workflow::read_input()?)?;
            println!("{}", report);
            if report["ok"] != true {
                io::stdout().flush()?;
                std::process::exit(2);
            }
            return Ok(());
        }
        _ => {}
    }
    if args.len() != 4 {
        bail!("无效参数；coverage 需要基线，draft 可指定功能 ID");
    }
    match action {
        "report" => {
            let event = serde_json::from_slice(&workflow::read_input()?)?;
            println!("{}", json!({"ok":true,"event":reader_core::journal::report(root,event)?}));
        }
        "inspect" => println!("{}", reader_core::agent::inspect(root)?),
        "deliver" => {
            let mut bytes = Vec::new();
            io::stdin()
                .take(2 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)?;
            if bytes.len() > 2 * 1024 * 1024 {
                bail!("交付 JSON 超出 2 MiB 限制");
            }
            let delivery = serde_json::from_slice(&bytes).context("无效的交付协议 JSON")?;
            let project = reader_core::agent::deliver(root, delivery)?;
            println!("{}", json!({"ok":true,"project":project}));
        }
        "check" => {
            let project = reader_core::open_project(root)?;
            let features: Vec<_> = project
                .features
                .iter()
                .filter(|f| f.agent_delivery.is_some())
                .collect();
            let valid = reader_core::agent::blocking_warnings(&project.warnings).is_empty()
                && features
                    .iter()
                    .all(|f| f.agent_delivery.as_ref().is_some_and(|d| d.matches_source));
            println!(
                "{}",
                json!({"ok":valid,"features":features,"warnings":project.warnings})
            );
            if !valid {
                io::stdout().flush()?;
                std::process::exit(2);
            }
        }
        _ => bail!("Unknown agent command; use inspect, deliver, check, draft, coverage <base> validate or report"),
    }
    Ok(())
}

fn run_trace(args: &[String]) -> Result<()> {
    use reader_core::trace;
    if args.len() != 4 {
        bail!("Usage: reader-cli trace <start|append|finish|list|read> <repository>; JSON from stdin except list");
    }
    let root = Path::new(&args[3]);
    let value = match args[2].as_str() {
        "start" => serde_json::to_value(trace::start(
            root,
            serde_json::from_slice(&workflow::read_input()?)?,
        )?)?,
        "append" => serde_json::to_value(trace::append(
            root,
            serde_json::from_slice(&workflow::read_input()?)?,
        )?)?,
        "finish" => serde_json::to_value(trace::finish(
            root,
            serde_json::from_slice(&workflow::read_input()?)?,
        )?)?,
        "list" => serde_json::to_value(trace::list(root)?)?,
        "read" => {
            let input: Value = serde_json::from_slice(&workflow::read_input()?)?;
            serde_json::to_value(trace::read(root, field(&input, "runId")?)?)?
        }
        _ => bail!("Unknown trace action"),
    };
    println!("{}", json!({"ok":true,"value":value}));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        println!("{}", json!({"ok": false, "error": format!("{error:#}")}));
        std::process::exit(1);
    }
}
