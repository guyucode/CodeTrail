use super::*;
use crate::types::{CodeRange, RelationKind};
use std::io::Cursor;

fn fixture() -> (
    BTreeMap<String, String>,
    Vec<Symbol>,
    Vec<CodeRelation>,
    Symbol,
) {
    let files: BTreeMap<String, String> = BTreeMap::from([
        ("a.py".into(), "class Greeter:\n    def greet(self, value):\n        return value\n\ndef run(value):\n    return value\n".into()),
        ("b.py".into(), "from a import Greeter, run as alias\ndef caller():\n    obj = Greeter()\n    note = '🐍'; return obj.greet('x')\n\ndef use_callback():\n    return alias\n".into()),
    ]);
    let symbols: Vec<_> = files
        .iter()
        .flat_map(|(path, text)| crate::parse::parse_symbols(path, text).unwrap())
        .collect();
    let focus = symbols
        .iter()
        .find(|symbol| symbol.id == "a.py::run")
        .unwrap()
        .clone();
    let line = files["b.py"].lines().nth(3).unwrap();
    let method_column = line[..line.find("greet").unwrap()].encode_utf16().count() as u32 + 1;
    let mut relations = vec![
        CodeRelation {
            id: "method-site".into(),
            kind: RelationKind::Call,
            source_symbol_id: Some("b.py::caller".into()),
            source_name: "caller".into(),
            target_symbol_id: None,
            target_name: "obj.greet".into(),
            site: CodeRange {
                file: "b.py".into(),
                start_line: 4,
                end_line: 4,
            },
            site_column: method_column,
            target: None,
            resolution: RelationResolution::Unresolved,
            origin: RelationOrigin::Syntax,
            detail: "dynamic receiver".into(),
        },
        CodeRelation {
            id: "reference-site".into(),
            kind: RelationKind::Reference,
            source_symbol_id: Some("b.py::use_callback".into()),
            source_name: "use_callback".into(),
            target_symbol_id: None,
            target_name: "alias".into(),
            site: CodeRange {
                file: "b.py".into(),
                start_line: 7,
                end_line: 7,
            },
            site_column: 12,
            target: None,
            resolution: RelationResolution::Unresolved,
            origin: RelationOrigin::Syntax,
            detail: "function value".into(),
        },
    ];
    let start = files["b.py"].find("greet('x')").unwrap();
    let end = start + "greet".len();
    let identity = format!("b.py\0{start}\0{end}\0call\0b.py::caller\0obj.greet");
    relations[0].id = format!("rel:{}", crate::digest(identity.as_bytes()));
    (files, symbols, relations, focus)
}

fn test_node(root: &Path) -> Option<PathBuf> {
    match find_node(root) {
        Ok(node)
            if node_version(&node)
                .and_then(|version| permission_flag(&version))
                .is_ok() =>
        {
            Some(node)
        }
        _ => {
            eprintln!("Skipping optional LSP process test: Node >= 22.12 is unavailable");
            None
        }
    }
}

const FAKE_SERVER: &str = r#"
const mode = MODE;
const fs = require('node:fs');
const {fileURLToPath} = require('node:url');
let input = Buffer.alloc(0), rootUri = '', pendingInitialize, documents = new Map();
const send = message => {const bytes = Buffer.from(JSON.stringify({jsonrpc:'2.0', ...message})); process.stdout.write(`Content-Length: ${bytes.length}\r\n\r\n`); process.stdout.write(bytes);};
const range = (line, start, end) => ({start:{line,character:start},end:{line,character:end}});
const fail = message => {console.error(message);process.exit(2);};
if (mode === 'oversized') process.stdout.write('Content-Length: 99999999999\r\n\r\n');
if (mode === 'stderr') process.stderr.write('x'.repeat(80 * 1024));
function handle(message) {
  if (message.id === 'configuration' && !message.method) {
    if (!message.result?.[0]?.pythonPath?.endsWith('disabled-python-interpreter')) fail('unsafe interpreter configuration');
    if (message.result?.[0]?.analysis?.autoSearchPaths !== false) fail('automatic search paths must remain disabled in LSP settings');
    send({id:pendingInitialize,result:{capabilities:{definitionProvider:true,referencesProvider: mode !== 'no-references',positionEncoding:'utf-16'}}}); return;
  }
  const params = message.params;
  if (message.method === 'initialize') {
    if (mode === 'hang') return;
    if (mode === 'flood') {setInterval(()=>send({method:'$/progress',params:{}}),1);return;}
    rootUri = params.rootUri; pendingInitialize = message.id;
    const root = fileURLToPath(rootUri);
    if (!root.includes('codetrail-semantic-')) fail('did not use private snapshot');
    const config = JSON.parse(fs.readFileSync(root + '/pyrightconfig.json','utf8'));
    if (config.extends || config.venv || config.pythonVersion !== '3.12') fail('project configuration escaped');
    if ('autoSearchPaths' in config) fail('autoSearchPaths is an LSP setting, not a pyrightconfig option');
    let subprocessBlocked=false, writeBlocked=false;
    try {require('node:child_process').execFileSync(process.execPath,['--version'])} catch(e) {subprocessBlocked=e.code === 'ERR_ACCESS_DENIED'}
    try {fs.writeFileSync(root + '/a.py','changed')} catch(e) {writeBlocked=e.code === 'ERR_ACCESS_DENIED'}
    if (!subprocessBlocked || !writeBlocked) fail('permissions were not enforced');
    send({id:'configuration',method:'workspace/configuration',params:{items:[{section:'python'}]}}); return;
  }
  if (message.method === 'textDocument/didOpen') {documents.set(params.textDocument.uri,params.textDocument.text);return;}
  if (message.method === 'textDocument/definition') {
    if (documents.size !== 2) fail('missing captured didOpen contents');
    if (mode === 'request-error') {send({id:message.id,error:{code:-32603,message:'fixture failure'}});return;}
    if (params.position.line === 3) {
      const line = documents.get(params.textDocument.uri).split('\n')[3];
      if (params.position.character !== line.indexOf('greet')) fail('UTF-16 site mismatch');
      send({id:message.id,result:[{targetUri:rootUri+'/a.py',targetRange:range(1,0,27),targetSelectionRange:range(1,8,13)}]});
    } else send({id:message.id,result:mode === 'ambiguous' ? [
      {uri:rootUri+'/a.py',range:range(4,4,7)},
      {uri:rootUri+'/a.py',range:range(1,8,13)}
    ] : [{uri:rootUri+'/a.py',range:range(4,4,7)}]});
    return;
  }
  if (message.method === 'textDocument/references') {
    if (params.position.line !== 4 || params.position.character !== 4) fail('wrong focus declaration');
    send({id:message.id,result:[
      {uri:rootUri+'/b.py',range:range(6,11,16)},
      {uri:rootUri+'/b.py',range:range(0,28,33)}
    ]});return;
  }
  if (message.method === 'shutdown') {send({id:message.id,result:null});return;}
  if (message.method === 'exit') process.exit(0);
}
process.stdin.on('data', bytes => {
  input = Buffer.concat([input,bytes]);
  while (true) {
    const end = input.indexOf('\r\n\r\n');if (end < 0) return;
    const match = /Content-Length:\s*(\d+)/i.exec(input.subarray(0,end).toString());if (!match) fail('bad input header');
    const size = Number(match[1]);if (input.length < end+4+size) return;
    const message = JSON.parse(input.subarray(end+4,end+4+size));input=input.subarray(end+4+size);handle(message);
  }
});
"#;

fn fake_command(directory: &Path, mode: &str) -> Option<ServerCommand> {
    let node = test_node(directory)?;
    let script = directory.join("fake-server.cjs");
    fs::write(
        &script,
        FAKE_SERVER.replace("MODE", &serde_json::to_string(mode).unwrap()),
    )
    .unwrap();
    Some(ServerCommand::new(node, script).unwrap())
}

#[test]
fn fake_lsp_exchanges_configuration_definitions_references_and_enforces_permissions() {
    let temp = tempfile::tempdir().unwrap();
    let Some(command) = fake_command(temp.path(), "normal") else {
        return;
    };
    let (files, symbols, mut relations, focus) = fixture();
    let report = enrich_using(
        &files,
        &symbols,
        &mut relations,
        &focus,
        command,
        Limits::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(report.engine.status, RelationEngineStatus::Ready);
    assert_eq!(
        relations[0].target_symbol_id.as_deref(),
        Some("a.py::Greeter::greet")
    );
    assert_eq!(relations[0].origin, RelationOrigin::Lsp);
    assert_eq!(relations[1].target_symbol_id.as_deref(), Some("a.py::run"));
    assert_eq!(relations[1].kind, RelationKind::Reference);
    assert!(
        !report
            .warnings
            .iter()
            .any(|warning| warning.contains("unrecognized setting")),
        "Pyright rejected the generated configuration: {:?}",
        report.warnings
    );
    assert_eq!(
        relations.len(),
        2,
        "references at import locations must not fabricate calls"
    );
    assert!(report.engine.detail.contains("references 返回 2 处"));
}

#[test]
fn hung_and_notification_flooding_servers_are_killed_within_budget() {
    for mode in ["hang", "flood"] {
        let temp = tempfile::tempdir().unwrap();
        let Some(command) = fake_command(temp.path(), mode) else {
            return;
        };
        let (files, symbols, mut relations, focus) = fixture();
        let original = relations.clone();
        let limits = Limits {
            total: Duration::from_millis(350),
            initialize: Duration::from_millis(250),
            request: Duration::from_millis(100),
            definitions: 8,
        };
        let started = Instant::now();
        let result = enrich_using(
            &files,
            &symbols,
            &mut relations,
            &focus,
            command,
            limits,
            started,
        );
        assert!(result.is_err(), "{mode} should fail");
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{mode} exceeded bounded shutdown"
        );
        assert_eq!(relations, original, "failure must preserve syntax results");
    }
}

#[test]
fn malformed_output_and_request_errors_leave_original_relations_untouched() {
    for mode in ["oversized", "stderr", "request-error"] {
        let temp = tempfile::tempdir().unwrap();
        let Some(command) = fake_command(temp.path(), mode) else {
            return;
        };
        let (files, symbols, mut relations, focus) = fixture();
        let original = relations.clone();
        let result = enrich_using(
            &files,
            &symbols,
            &mut relations,
            &focus,
            command,
            Limits::default(),
            Instant::now(),
        );
        assert!(result.is_err(), "{mode} should fail");
        assert_eq!(relations, original);
        let report = unavailable(result.err().unwrap());
        assert_eq!(report.engine.status, RelationEngineStatus::Unavailable);
        assert!(!report.warnings.is_empty());
    }
}

#[test]
fn missing_references_capability_and_request_caps_report_partial() {
    let temp = tempfile::tempdir().unwrap();
    let Some(command) = fake_command(temp.path(), "no-references") else {
        return;
    };
    let (files, symbols, mut relations, focus) = fixture();
    let report = enrich_using(
        &files,
        &symbols,
        &mut relations,
        &focus,
        command,
        Limits {
            definitions: 1,
            ..Limits::default()
        },
        Instant::now(),
    )
    .unwrap();
    assert_eq!(report.engine.status, RelationEngineStatus::Partial);
    assert_eq!(
        relations
            .iter()
            .filter(|relation| relation.origin == RelationOrigin::Lsp)
            .count(),
        1
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("references")));
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("最多核对")));
}

#[test]
fn references_do_not_erase_a_multiple_definition_ambiguity() {
    let temp = tempfile::tempdir().unwrap();
    let Some(command) = fake_command(temp.path(), "ambiguous") else {
        return;
    };
    let (files, symbols, mut relations, focus) = fixture();
    let report = enrich_using(
        &files,
        &symbols,
        &mut relations,
        &focus,
        command,
        Limits::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(report.engine.status, RelationEngineStatus::Partial);
    assert_eq!(relations[1].resolution, RelationResolution::Ambiguous);
    assert_eq!(relations[1].origin, RelationOrigin::Lsp);
    assert!(relations[1].target_symbol_id.is_none());
}

#[test]
fn explicit_binding_invalidation_blocks_both_definition_and_reference_upgrades() {
    let temp = tempfile::tempdir().unwrap();
    let Some(command) = fake_command(temp.path(), "normal") else {
        return;
    };
    let (files, symbols, mut relations, focus) = fixture();
    let original = relations.clone();
    let blocked_relation_ids = relations
        .iter()
        .map(|relation| relation.id.clone())
        .collect();
    let report = enrich_using_policy(
        EnrichmentInputs {
            files: &files,
            symbols: &symbols,
            relations: &mut relations,
            focus: &focus,
            blocked_relation_ids: &blocked_relation_ids,
        },
        command,
        Limits::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(report.engine.status, RelationEngineStatus::Partial);
    assert_eq!(
        relations, original,
        "LSP facts cannot undo observed dynamic binding writes"
    );
    assert!(report
        .warnings
        .iter()
        .any(|warning| warning.contains("绑定失效")));
}

#[test]
fn definition_inside_parameter_must_not_resolve_to_its_containing_function() {
    let (files, symbols, mut relations, _) = fixture();
    let snapshot = Snapshot::new(&files).unwrap();
    let declarations = declarations(&snapshot, &symbols).unwrap();
    let result = locations(&json!([{"uri": snapshot.uris["a.py"], "range": {
        "start":{"line":4,"character":8},"end":{"line":4,"character":13}
    }}]))
    .unwrap();
    resolve_definition(
        &mut relations[0],
        &result,
        &snapshot,
        &declarations,
        &symbols,
    );
    assert_eq!(relations[0].resolution, RelationResolution::Unresolved);
    assert!(relations[0].target_symbol_id.is_none());
    assert!(relations[0].detail.contains("参数"));
}

#[test]
fn multiple_and_external_definitions_remain_conservative() {
    let (files, symbols, mut relations, _) = fixture();
    let snapshot = Snapshot::new(&files).unwrap();
    let declarations = declarations(&snapshot, &symbols).unwrap();
    let result = locations(&json!([
        {"uri":snapshot.uris["a.py"],"range":{"start":{"line":4,"character":4},"end":{"line":4,"character":7}}},
        {"uri":snapshot.uris["a.py"],"range":{"start":{"line":1,"character":8},"end":{"line":1,"character":13}}}
    ])).unwrap();
    resolve_definition(
        &mut relations[0],
        &result,
        &snapshot,
        &declarations,
        &symbols,
    );
    assert_eq!(relations[0].resolution, RelationResolution::Ambiguous);
    assert!(relations[0].target_symbol_id.is_none());
    let external = locations(&json!([{"uri":"file:///outside/stdlib.py","range":{"start":{"line":1,"character":1},"end":{"line":1,"character":8}}}])).unwrap();
    resolve_definition(
        &mut relations[1],
        &external,
        &snapshot,
        &declarations,
        &symbols,
    );
    assert_eq!(relations[1].resolution, RelationResolution::External);
    assert!(relations[1].target.is_none());
}

#[test]
fn framing_and_snapshot_path_limits_fail_closed() {
    for frame in [
        b"Content-Length: 3000000\r\n\r\n".as_slice(),
        b"Content-Length: 2\r\nContent-Length: 2\r\n\r\n{}".as_slice(),
        b"nonsense\r\n\r\n".as_slice(),
        b"Content-Length: 1\r\n\r\nx".as_slice(),
    ] {
        assert!(read_message(&mut Cursor::new(frame), &mut 0).is_err());
    }
    let frame =
        b"content-length: 2\r\nContent-Type: application/vscode-jsonrpc; charset=utf-8\r\n\r\n{}";
    assert_eq!(
        read_message(&mut Cursor::new(frame), &mut 0).unwrap(),
        json!({})
    );
    for path in [
        "../escape.py",
        "/absolute.py",
        "a/../../escape.py",
        ".git/hidden.py",
        "a\\b.py",
    ] {
        assert!(
            Snapshot::new(&BTreeMap::from([(path.into(), "pass\n".into())])).is_err(),
            "{path}"
        );
    }
    assert!(Snapshot::new(&BTreeMap::from([(
        "large.py".into(),
        " ".repeat(MAX_FILE_BYTES + 1)
    )]))
    .is_err());
}

#[test]
fn uris_round_trip_spaces_unicode_and_reject_remote_authorities() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("空 格 # %.py");
    let uri = file_uri(&path).unwrap();
    assert!(uri.contains("%20") && uri.contains("%23") && uri.contains("%25"));
    assert_eq!(decode_file_uri(&uri), Some(path));
    assert!(decode_file_uri("file://malicious-host/private.py").is_none());
    assert!(decode_file_uri("file:///tmp/a.py%00").is_none());
    assert_eq!(
        permission_flag("v22.12.0\n").unwrap(),
        "--experimental-permission"
    );
    assert_eq!(permission_flag("v24.22.1\n").unwrap(), "--permission");
    assert!(permission_flag("v20.19.0").is_err());
}

struct DynamicFixture {
    _root: TempDir,
    files: BTreeMap<String, String>,
    symbols: Vec<Symbol>,
    relations: Vec<CodeRelation>,
    focus: Symbol,
}

fn dynamic_fixture() -> DynamicFixture {
    // The long identifier makes the 180-character display prefixes of an inner
    // and outer callee identical. Only the full byte identity distinguishes it.
    let long_name = format!("factory_{}", "x".repeat(190));
    let text = format!("def leaf():\n    return 1\n\ndef factory():\n    return leaf\n\ndef {long_name}():\n    return leaf\n\ndef caller(functions, index, flag):\n    factory()()\n    functions[index]()\n    (factory if flag else leaf)()\n    {long_name}()()\n    return factory()\n");
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.py"), &text).unwrap();
    let symbols = crate::parse::parse_symbols("a.py", &text).unwrap();
    let focus = symbols
        .iter()
        .find(|symbol| symbol.id == "a.py::factory")
        .unwrap()
        .clone();
    let view =
        crate::relations::analyze(root.path(), "a.py::caller", "HEAD", "WORKTREE", false).unwrap();
    DynamicFixture {
        _root: root,
        files: BTreeMap::from([("a.py".into(), text)]),
        symbols,
        relations: view.outgoing,
        focus,
    }
}

#[test]
fn ast_gate_distinguishes_outer_calls_even_when_locations_and_display_prefixes_match() {
    let fixture = dynamic_fixture();
    let snapshot = Snapshot::new(&fixture.files).unwrap();
    let eligible = eligible_sites(&snapshot, &fixture.relations).unwrap();
    assert_eq!(fixture.relations.len(), 7, "{:?}", fixture.relations);
    assert_eq!(eligible.iter().filter(|allowed| **allowed).count(), 3);
    for (relation, allowed) in fixture.relations.iter().zip(eligible) {
        if !allowed {
            assert_eq!(relation.resolution, RelationResolution::Unresolved);
            assert!(relation.target_symbol_id.is_none());
        }
    }
}

#[test]
fn real_pyright_never_rebinds_dynamic_return_subscript_or_conditional_calls() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/pyright/langserver.index.js");
    if !package.is_file() {
        eprintln!("Skipping optional real Pyright fixture: npm ci required");
        return;
    }
    let mut fixture = dynamic_fixture();
    let Some(node) = test_node(fixture._root.path()) else {
        return;
    };
    let command = ServerCommand::new(node, package.canonicalize().unwrap()).unwrap();
    let snapshot = Snapshot::new(&fixture.files).unwrap();
    let eligible = eligible_sites(&snapshot, &fixture.relations).unwrap();
    let original = fixture.relations.clone();
    let report = enrich_using(
        &fixture.files,
        &fixture.symbols,
        &mut fixture.relations,
        &fixture.focus,
        command,
        Limits::default(),
        Instant::now(),
    )
    .unwrap();
    assert_eq!(report.engine.status, RelationEngineStatus::Partial);
    for ((before, after), allowed) in original.iter().zip(&fixture.relations).zip(eligible) {
        if allowed {
            assert_eq!(
                after.origin,
                RelationOrigin::Lsp,
                "actual inner call should be enriched: {after:?}"
            );
            assert_eq!(after.resolution, RelationResolution::Resolved);
        } else {
            assert_eq!(before, after, "dynamic outer call was incorrectly upgraded");
        }
    }
    assert_eq!(
        fixture
            .relations
            .iter()
            .filter(|relation| relation.target_symbol_id.as_deref() == Some("a.py::factory"))
            .count(),
        2,
        "only the inner factory() and the direct factory() are calls to factory"
    );
}

#[test]
fn real_pyright_resolves_cross_file_method_and_function_value_without_running_source() {
    let package = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../node_modules/pyright/langserver.index.js");
    if !package.is_file() {
        eprintln!(
            "Skipping real Pyright fixture: run npm ci to install the application dependency"
        );
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let Some(node) = test_node(root.path()) else {
        return;
    };
    let command = ServerCommand::new(node, package.canonicalize().unwrap()).unwrap();
    let (mut files, symbols, mut relations, focus) = fixture();
    // Neither the repository config nor its disk content is input to the server.
    fs::write(
        root.path().join("pyrightconfig.json"),
        "{\"extends\":\"/not-allowed/config.json\",\"venv\":\"malicious\"}",
    )
    .unwrap();
    fs::write(
        root.path().join("a.py"),
        "raise RuntimeError('disk source must not be used')\n",
    )
    .unwrap();
    let before = fs::read(root.path().join("a.py")).unwrap();
    let tripwire = root.path().join("PYTHON_WAS_EXECUTED");
    files.insert(
        "tripwire.py".into(),
        format!(
            "from pathlib import Path\nPath({:?}).write_text('executed')\n",
            tripwire.to_string_lossy()
        ),
    );
    let report = enrich_using(
        &files,
        &symbols,
        &mut relations,
        &focus,
        command,
        Limits::default(),
        Instant::now(),
    )
    .unwrap();
    assert_ne!(
        report.engine.status,
        RelationEngineStatus::Unavailable,
        "{:?}",
        report.warnings
    );
    assert_eq!(
        relations[0].target_symbol_id.as_deref(),
        Some("a.py::Greeter::greet"),
        "{relations:#?}; engine={:?}; warnings={:?}",
        report.engine,
        report.warnings
    );
    assert_eq!(
        relations[1].target_symbol_id.as_deref(),
        Some("a.py::run"),
        "{relations:#?}; engine={:?}; warnings={:?}",
        report.engine,
        report.warnings
    );
    assert_eq!(relations[1].kind, RelationKind::Reference);
    assert!(relations
        .iter()
        .all(|relation| relation.origin == RelationOrigin::Lsp));
    assert_eq!(fs::read(root.path().join("a.py")).unwrap(), before);
    assert!(
        !tripwire.exists(),
        "language-server analysis executed Python source"
    );
}

#[cfg(windows)]
#[test]
fn verbatim_windows_paths_round_trip_inside_the_private_snapshot() {
    let (files, _, _, _) = fixture();
    let snapshot = Snapshot::new(&files).unwrap();
    let canonical = snapshot.workspace.join("a.py").canonicalize().unwrap();
    let uri = file_uri(&canonical).unwrap();
    assert!(uri.starts_with("file:///"));
    assert!(!uri.contains("%3F"));
    assert_eq!(snapshot.relative_uri(&uri), Some("a.py".into()));
    assert!(!permission_path("--allow-fs-read=", &canonical)
        .unwrap()
        .contains(r"\\?\"));
    assert!(decode_file_uri("file:////remote-host/share/file.py").is_none());
}
