//! Optional, bounded Python LSP enrichment of already classified AST sites.
//!
//! Pyright sees a private copy of the caller's captured text, never the original
//! repository or its configuration. The caller must still revalidate its source
//! fingerprint before accepting this result. A language server is not a runtime
//! execution trace, and reference locations are never reclassified as calls.

use crate::types::{
    CodeRelation, RelationEngine, RelationEngineStatus, RelationKind, RelationOrigin,
    RelationResolution, Symbol,
};
use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tree_sitter::{Node, Parser};

const MAX_PYTHON_FILES: usize = 256;
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;
const MAX_FILE_BYTES: usize = 2 * 1024 * 1024;
const MAX_DEFINITIONS: usize = 256;
const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 8 * 1024 * 1024;
const MAX_STDERR_BYTES: usize = 64 * 1024;
const MAX_HEADER_BYTES: usize = 8192;
const MAX_MESSAGES: usize = 4096;

static BUNDLED_PYRIGHT: OnceLock<PathBuf> = OnceLock::new();

/// Set by the application from its trusted resource directory, not project data.
/// First registration wins; normal analysis cannot replace the language server.
pub fn set_bundled_pyright(path: PathBuf) {
    let _ = BUNDLED_PYRIGHT.set(path);
}

pub struct SemanticReport {
    pub engine: RelationEngine,
    pub warnings: Vec<String>,
}

#[derive(Clone, Copy)]
struct Limits {
    total: Duration,
    initialize: Duration,
    request: Duration,
    definitions: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            total: Duration::from_secs(15),
            initialize: Duration::from_secs(6),
            request: Duration::from_secs(2),
            definitions: MAX_DEFINITIONS,
        }
    }
}

/// Enrich only captured Python AST sites. Failure leaves every input relation
/// intact. Incomplete but valid bounded results explicitly report `Partial`.
pub fn enrich(
    root: &Path,
    files: &BTreeMap<String, String>,
    symbols: &[Symbol],
    relations: &mut [CodeRelation],
    focus: &Symbol,
) -> SemanticReport {
    enrich_with_policy(root, files, symbols, relations, focus, &BTreeSet::new())
}

/// Preserve the core analyzer's explicit invalidation evidence. A declaration
/// location cannot restore a binding known to have been dynamically rewritten.
pub fn enrich_with_policy(
    root: &Path,
    files: &BTreeMap<String, String>,
    symbols: &[Symbol],
    relations: &mut [CodeRelation],
    focus: &Symbol,
    blocked_relation_ids: &BTreeSet<String>,
) -> SemanticReport {
    let started = Instant::now();
    let result = ServerCommand::discover(root).and_then(|command| {
        let root = root.canonicalize()?;
        if std::env::temp_dir().canonicalize()?.starts_with(&root) {
            bail!("系统临时目录位于被阅读仓库内，不能在该仓库创建语义副本");
        }
        enrich_using_policy(
            EnrichmentInputs {
                files,
                symbols,
                relations,
                focus,
                blocked_relation_ids,
            },
            command,
            Limits::default(),
            started,
        )
    });
    match result {
        Ok(report) => report,
        Err(error) => unavailable(error),
    }
}

fn unavailable(error: anyhow::Error) -> SemanticReport {
    let reason: String = format!("{error:#}").chars().take(700).collect();
    SemanticReport {
        engine: RelationEngine {
            name: "Python AST（Pyright 不可用）".into(),
            status: RelationEngineStatus::Unavailable,
            detail: "保留语法关系；本次未应用语言服务器结果。".into(),
        },
        warnings: vec![format!("Pyright 语义增强不可用，已保留语法分析：{reason}")],
    }
}

struct ServerCommand {
    node: PathBuf,
    script: PathBuf,
    permission: &'static str,
}

impl ServerCommand {
    fn discover(root: &Path) -> Result<Self> {
        let root = root.canonicalize().context("无法确认项目路径")?;
        let node = find_node(&root)?;
        let script = if let Some(path) = std::env::var_os("CODETRAIL_PYRIGHT_SCRIPT") {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                bail!("CODETRAIL_PYRIGHT_SCRIPT 必须是应用级 Pyright 脚本的绝对路径");
            }
            let canonical = path.canonicalize().context("指定的 Pyright 脚本不存在")?;
            if canonical.starts_with(&root) {
                bail!("不运行被阅读仓库中的环境覆盖脚本；请使用独立安装的 Pyright");
            }
            canonical
        } else if let Some(path) = BUNDLED_PYRIGHT.get() {
            path.canonicalize()
                .context("应用捆绑的 Pyright 资源不存在")?
        } else {
            // This is the CodeTrail build location, never `root` or the process
            // working directory. Packaged applications register their resource.
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../node_modules/pyright/langserver.index.js")
                .canonicalize()
                .context("未找到应用依赖 Pyright；请在 CodeTrail 目录运行 npm ci")?
        };
        Self::new(node, script)
    }

    fn new(node: PathBuf, script: PathBuf) -> Result<Self> {
        if !node.is_absolute() || !node.is_file() || !script.is_absolute() || !script.is_file() {
            bail!("Node 与 Pyright 必须是存在的绝对文件路径");
        }
        if !matches!(
            script.extension().and_then(OsStr::to_str),
            Some("js" | "cjs")
        ) {
            bail!("Pyright 入口必须为 JavaScript 文件，不接受 shell 命令或包装脚本");
        }
        let version = node_version(&node)?;
        let permission = permission_flag(&version)?;
        Ok(Self {
            node,
            script,
            permission,
        })
    }

    fn command(&self, snapshot: &Snapshot) -> Result<Command> {
        let package = self.script.parent().context("Pyright 入口没有父目录")?;
        let bootstrap = NODE_BOOTSTRAP.to_owned();
        #[cfg(all(test, windows))]
        let bootstrap = format!("{NODE_FS_DIAGNOSTICS}\n{bootstrap}");
        let mut command = Command::new(&self.node);
        command
            .env_clear()
            .current_dir(&snapshot.workspace)
            .env("TMPDIR", portable_path(&snapshot.state)?)
            .env("TMP", portable_path(&snapshot.state)?)
            .env("TEMP", portable_path(&snapshot.state)?)
            .arg(self.permission)
            .arg(permission_path("--allow-fs-read=", package)?)
            .arg(permission_path(
                "--allow-fs-read=",
                snapshot.workspace.parent().context("快照根目录不存在")?,
            )?)
            .arg(permission_path("--allow-fs-write=", &snapshot.state)?)
            .arg("--max-old-space-size=256")
            .arg("-e")
            .arg(bootstrap)
            .arg(portable_path(&self.script)?)
            .arg("--stdio");
        // Deliberately no child-process, worker, native-addon or WASI grant.
        // `env_clear` also removes NODE_OPTIONS/PYTHONPATH/venv and preload hooks.
        #[cfg(windows)]
        if let Some(system_root) = std::env::var_os("SystemRoot") {
            command.env("SystemRoot", system_root);
        }
        Ok(command)
    }
}

// Pyright's bundled Yarn path helpers extend `path.posix`; Node permission mode
// freezes the real path built-in. Return private copies to application imports,
// preserving Node's frozen originals and all fs/process permission checks.
// Only this trusted application bootstrap and Pyright are evaluated as JS.
const NODE_BOOTSTRAP: &str = r#"
process.on('uncaughtException', error => {
  console.error('Pyright startup: ' + String(error.message).slice(0,900));
  process.exit(1);
});
const Module = require('node:module');
const nativePath = require('node:path');
const originalLoad = Module._load;
const pathCopy = {...nativePath};
const posixCopy = {...nativePath.posix};
const win32Copy = {...nativePath.win32};
for (const value of [pathCopy, posixCopy, win32Copy]) {
  value.posix = posixCopy;
  value.win32 = win32Copy;
}
Module._load = function (name, parent, isMain) {
  if (name === 'path' || name === 'node:path') return pathCopy;
  return originalLoad.call(this, name, parent, isMain);
};
require(process.argv[1]);
"#;

#[cfg(all(test, windows))]
const NODE_FS_DIAGNOSTICS: &str = r#"
const diagnosticModule = require('node:module');
const diagnosticLoad = diagnosticModule._load;
const diagnosticFs = require('node:fs');
let diagnosticCount = 0;
const diagnosticProxy = new Proxy(diagnosticFs, {get(target, name) {
  const value = target[name];
  if (!['readFileSync','readdirSync','statSync','existsSync','realpathSync'].includes(name)) return value;
  return function (...args) {
    try {
      const result = value.apply(target,args);
      if (name === 'existsSync' && !result && diagnosticCount++ < 30) console.error('FS missing: '+String(args[0]));
      return result;
    } catch (error) {
      if (diagnosticCount++ < 30) console.error('FS '+name+': '+String(args[0])+' '+error.code);
      throw error;
    }
  };
}});
diagnosticModule._load = function (name, parent, isMain) {
  if (name === 'fs' || name === 'node:fs') return diagnosticProxy;
  return diagnosticLoad.call(this,name,parent,isMain);
};
"#;

fn permission_path(prefix: &str, path: &Path) -> Result<String> {
    let path = portable_path(path)?;
    let text = path.to_str().context("语义工具路径不是 UTF-8")?;
    // Node treats these characters as permission-list separators/wildcards.
    if text.contains([',', '*', '\n', '\r']) {
        bail!("语义工具安装路径或临时路径含 Node 权限参数不支持的字符");
    }
    Ok(format!("{prefix}{text}"))
}

fn find_node(root: &Path) -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("CODETRAIL_NODE") {
        let path = PathBuf::from(path);
        if !path.is_absolute() {
            bail!("CODETRAIL_NODE 必须是 Node 可执行文件的绝对路径");
        }
        let path = path
            .canonicalize()
            .context("CODETRAIL_NODE 指定的文件不存在")?;
        if path.starts_with(root) || !path.is_file() {
            bail!("不运行被阅读仓库内的 Node 可执行文件");
        }
        return Ok(path);
    }
    let executable = if cfg!(windows) { "node.exe" } else { "node" };
    for directory in std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()) {
        // Empty/relative PATH entries may be controlled by the opened project.
        if !directory.is_absolute() {
            continue;
        }
        let Ok(path) = directory.join(executable).canonicalize() else {
            continue;
        };
        if path.starts_with(root) || !path.is_file() {
            continue;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(&path)?.permissions().mode() & 0o111 == 0 {
                continue;
            }
        }
        return Ok(path);
    }
    bail!("找不到可信 PATH 中的 Node；安装 Node >= 22.12 或设置 CODETRAIL_NODE 绝对路径")
}

fn node_version(node: &Path) -> Result<String> {
    let mut child = Command::new(node)
        .arg("--version")
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("无法启动 Node 版本探测")?;
    let mut stdout = child.stdout.take().context("Node stdout 不可用")?;
    let (tx, rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout.by_ref().take(129).read_to_end(&mut bytes);
        let _ = tx.send((result, bytes));
    });
    let received = rx.recv_timeout(Duration::from_secs(2));
    let status = child.try_wait();
    if !matches!(status, Ok(Some(_))) {
        let _ = child.kill();
    }
    let _ = child.wait();
    let _ = reader.join();
    let (read_result, bytes) = received.context("Node 版本探测超时")?;
    read_result.context("Node 版本读取失败")?;
    if bytes.len() > 128 {
        bail!("Node 版本输出超过上限");
    }
    String::from_utf8(bytes).context("Node 版本输出无效")
}

fn permission_flag(version: &str) -> Result<&'static str> {
    let mut parts = version.trim().strip_prefix('v').unwrap_or("").split('.');
    let major = parts.next().and_then(|part| part.parse::<u32>().ok());
    let minor = parts.next().and_then(|part| part.parse::<u32>().ok());
    match (major, minor) {
        (Some(22), Some(12)) | (Some(23), Some(0..=4)) => Ok("--experimental-permission"),
        (Some(22), Some(13..)) | (Some(23), Some(5..)) | (Some(24..), Some(_)) => {
            Ok("--permission")
        }
        _ => bail!(
            "语义增强需要支持权限控制的 Node >= 22.12；当前为 {}",
            version.trim()
        ),
    }
}

struct Snapshot {
    _temp: TempDir,
    workspace: PathBuf,
    state: PathBuf,
    files: BTreeMap<String, String>,
    uris: BTreeMap<String, String>,
}

impl Snapshot {
    fn new(files: &BTreeMap<String, String>) -> Result<Self> {
        let python_files: Vec<_> = files
            .iter()
            .filter(|(path, _)| crate::parse::language_for(path) == "python")
            .collect();
        if python_files.is_empty() {
            bail!("捕获内容中没有 Python 文件");
        }
        if python_files.len() > MAX_PYTHON_FILES
            || python_files
                .iter()
                .map(|(_, text)| text.len())
                .sum::<usize>()
                > MAX_SOURCE_BYTES
        {
            bail!("语义快照超过 256 个 Python 文件或 16 MiB 上限，保留语法分析");
        }
        let python: BTreeMap<_, _> = python_files
            .into_iter()
            .map(|(path, text)| (path.clone(), text.clone()))
            .collect();
        let temp = tempfile::Builder::new()
            .prefix("codetrail-semantic-")
            .tempdir()?;
        let workspace = temp.path().join("workspace");
        let state = temp.path().join("state");
        fs::create_dir(&workspace)?;
        fs::create_dir(&state)?;
        let workspace = portable_path(&workspace.canonicalize()?)?;
        let state = portable_path(&state.canonicalize()?)?;
        let mut uris = BTreeMap::new();
        for (path, text) in &python {
            validate_relative(path)?;
            if text.len() > MAX_FILE_BYTES || text.contains('\0') {
                bail!("Python 文件超出 2 MiB 上限或含 NUL：{path}");
            }
            let destination = workspace.join(path);
            fs::create_dir_all(destination.parent().context("快照路径无父目录")?)?;
            fs::write(&destination, text)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(&destination, fs::Permissions::from_mode(0o600))?;
            }
            uris.insert(path.clone(), file_uri(&destination)?);
        }
        fs::write(
            workspace.join("pyrightconfig.json"),
            serde_json::to_vec(&json!({
                "include": ["**/*.py", "**/*.pyi"],
                "exclude": [],
                "pythonVersion": "3.12",
                "typeCheckingMode": "off",
                "useLibraryCodeForTypes": false,
                "extraPaths": []
            }))?,
        )?;
        Ok(Self {
            _temp: temp,
            workspace,
            state,
            files: python,
            uris,
        })
    }

    fn relative_uri(&self, uri: &str) -> Option<String> {
        let path = portable_path(&decode_file_uri(uri)?.canonicalize().ok()?).ok()?;
        let relative = path.strip_prefix(&self.workspace).ok()?;
        if relative
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
        {
            return None;
        }
        let relative = relative.to_str()?.replace('\\', "/");
        self.files.contains_key(&relative).then_some(relative)
    }

    fn configuration(&self, section: &str) -> Value {
        let analysis = json!({
            "diagnosticMode": "openFilesOnly", "autoSearchPaths": false,
            "useLibraryCodeForTypes": false, "autoImportCompletions": false,
            "typeCheckingMode": "off", "logLevel": "Error", "extraPaths": []
        });
        let python = json!({
            "pythonPath": self.state.join("disabled-python-interpreter"),
            "analysis": analysis
        });
        let pyright = json!({"disableOrganizeImports": true, "disableTaggedHints": true});
        match section {
            "python" => python,
            "python.analysis" => analysis,
            "pyright" => pyright,
            "" => json!({"python": python, "pyright": pyright}),
            _ => Value::Null,
        }
    }
}

fn validate_relative(path: &str) -> Result<()> {
    if path.is_empty() || path.contains(['\\', '\0']) || Path::new(path).is_absolute() {
        bail!("无效语义快照路径");
    }
    for part in Path::new(path).components() {
        let Component::Normal(value) = part else {
            bail!("语义快照路径不能越过项目边界")
        };
        if value == ".git" || value == ".reader" {
            bail!("语义快照不包含仓库管理目录");
        }
    }
    Ok(())
}

// Rust's Windows canonical paths use the device namespace (\\?\C:\...).
// LSP file URIs and Node permission arguments must use ordinary drive paths.
// Network/device authorities are not granted to the isolated language service.
fn portable_path(path: &Path) -> Result<PathBuf> {
    #[cfg(windows)]
    {
        let text = path.to_str().context("语义工具路径不是 UTF-8")?;
        if let Some(tail) = text.strip_prefix(r"\\?\") {
            if tail
                .get(..4)
                .is_some_and(|p| p.eq_ignore_ascii_case("UNC\\"))
            {
                bail!("语义增强暂不支持 UNC 网络路径");
            }
            return Ok(PathBuf::from(tail));
        }
        if text.starts_with(r"\\") {
            bail!("语义增强暂不支持 UNC 网络路径");
        }
    }
    Ok(path.to_path_buf())
}

fn file_uri(path: &Path) -> Result<String> {
    if !path.is_absolute() {
        bail!("LSP 文件 URI 必须使用绝对路径")
    }
    let path = portable_path(path)?;
    let path = path
        .to_str()
        .context("LSP 路径不是 UTF-8")?
        .replace('\\', "/");
    let mut result = if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    }
    .to_owned();
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte) {
            result.push(char::from(byte));
        } else {
            result.push_str(&format!("%{byte:02X}"));
        }
    }
    Ok(result)
}

fn decode_file_uri(uri: &str) -> Option<PathBuf> {
    let tail = uri.strip_prefix("file://")?;
    let tail = tail
        .strip_prefix("localhost/")
        .map(|v| format!("/{v}"))
        .unwrap_or_else(|| tail.to_owned());
    if !tail.starts_with('/') || tail.contains(['?', '#', '\0']) {
        return None;
    }
    let input = tail.as_bytes();
    let mut bytes = Vec::with_capacity(input.len());
    let mut index = 0;
    while index < input.len() {
        if input[index] == b'%' {
            let pair = input.get(index + 1..index + 3)?;
            let value = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
            if value == 0 {
                return None;
            }
            bytes.push(value);
            index += 3;
        } else {
            bytes.push(input[index]);
            index += 1;
        }
    }
    let text = String::from_utf8(bytes).ok()?;
    #[cfg(windows)]
    if text.starts_with("//") || text.starts_with(r"\\") {
        return None;
    }
    #[cfg(windows)]
    let text = if text.as_bytes().get(2) == Some(&b':') {
        text[1..].to_owned()
    } else {
        text
    };
    Some(PathBuf::from(text))
}

struct LspClient {
    child: Arc<Mutex<Child>>,
    stdin: Option<ChildStdin>,
    incoming: mpsc::Receiver<std::result::Result<Value, String>>,
    stop: Option<mpsc::Sender<()>>,
    threads: Vec<JoinHandle<()>>,
    deadline: Instant,
    next_id: u64,
    warnings: Vec<String>,
}

impl LspClient {
    fn start(mut command: Command, deadline: Instant) -> Result<Self> {
        if Instant::now() >= deadline {
            bail!("语义分析总时间预算已用尽")
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("无法启动 Pyright")?;
        let stdin = child.stdin.take().context("Pyright stdin 不可用")?;
        let stdout = child.stdout.take().context("Pyright stdout 不可用")?;
        let mut stderr = child.stderr.take().context("Pyright stderr 不可用")?;
        let child = Arc::new(Mutex::new(child));
        let (sender, incoming) = mpsc::channel();
        let output_sender = sender.clone();
        let output_reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut total = 0;
            for _ in 0..MAX_MESSAGES {
                match read_message(&mut reader, &mut total) {
                    Ok(message) => {
                        if output_sender.send(Ok(message)).is_err() {
                            return;
                        }
                    }
                    Err(error) => {
                        let _ = output_sender.send(Err(error.to_string()));
                        return;
                    }
                }
            }
            let _ = output_sender.send(Err("Pyright 消息数量超过上限".into()));
        });
        let error_reader = thread::spawn(move || {
            let mut total = 0;
            let mut buffer = [0_u8; 4096];
            loop {
                match stderr.read(&mut buffer) {
                    Ok(0) => return,
                    Ok(count) => {
                        #[cfg(all(test, windows))]
                        eprintln!(
                            "Pyright stderr: {}",
                            String::from_utf8_lossy(&buffer[..count])
                        );
                        total += count;
                        if total > MAX_STDERR_BYTES {
                            let _ = sender.send(Err("Pyright stderr 超过 64 KiB 上限".into()));
                            return;
                        }
                    }
                    Err(_) => return,
                }
            }
        });
        let (stop, stopped) = mpsc::channel();
        let watched = Arc::clone(&child);
        let watchdog = thread::spawn(move || {
            if stopped
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .is_err()
            {
                if let Ok(mut child) = watched.lock() {
                    let _ = child.kill();
                }
            }
        });
        Ok(Self {
            child,
            stdin: Some(stdin),
            incoming,
            stop: Some(stop),
            threads: vec![output_reader, error_reader, watchdog],
            deadline,
            next_id: 1,
            warnings: Vec::new(),
        })
    }

    fn send(&mut self, message: Value) -> Result<()> {
        if Instant::now() >= self.deadline {
            bail!("Pyright 总分析时间超过 15 秒上限")
        }
        let bytes = serde_json::to_vec(&message)?;
        // A maximum-sized source is JSON-escaped; allow bounded expansion.
        if bytes.len() > MAX_FILE_BYTES * 6 + 4096 {
            bail!("LSP 请求体过大")
        }
        let stdin = self.stdin.as_mut().context("Pyright 已关闭")?;
        write!(stdin, "Content-Length: {}\r\n\r\n", bytes.len())?;
        stdin.write_all(&bytes)?;
        stdin.flush()?;
        Ok(())
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    fn request(
        &mut self,
        method: &str,
        params: Value,
        timeout: Duration,
        snapshot: &Snapshot,
    ) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = self.deadline.min(Instant::now() + timeout);
        loop {
            let wait = deadline.saturating_duration_since(Instant::now());
            if wait.is_zero() {
                bail!("Pyright 请求超时：{method}")
            }
            let message = self
                .incoming
                .recv_timeout(wait)
                .with_context(|| format!("Pyright 请求超时或进程已结束：{method}"))?
                .map_err(|error| anyhow!(error))?;
            if let Some(incoming_method) = message.get("method").and_then(Value::as_str) {
                #[cfg(all(test, windows))]
                if matches!(
                    incoming_method,
                    "window/logMessage" | "textDocument/publishDiagnostics"
                ) {
                    eprintln!("Pyright notification: {message}");
                }
                if let Some(request_id) = message.get("id") {
                    self.answer_request(incoming_method, request_id.clone(), &message, snapshot)?;
                } else if matches!(incoming_method, "window/logMessage" | "window/showMessage")
                    && message
                        .pointer("/params/type")
                        .and_then(Value::as_u64)
                        .is_some_and(|kind| kind <= 2)
                    && self.warnings.len() < 5
                {
                    if let Some(text) = message.pointer("/params/message").and_then(Value::as_str) {
                        self.warnings.push(format!(
                            "Pyright：{}",
                            text.chars().take(240).collect::<String>()
                        ));
                    }
                }
                continue;
            }
            if message.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(error) = message.get("error") {
                    bail!(
                        "Pyright {method} 返回错误：{}",
                        error.to_string().chars().take(350).collect::<String>()
                    );
                }
                return message
                    .get("result")
                    .cloned()
                    .context("LSP 响应缺少 result");
            }
        }
    }

    fn answer_request(
        &mut self,
        method: &str,
        id: Value,
        message: &Value,
        snapshot: &Snapshot,
    ) -> Result<()> {
        let result = match method {
            "workspace/configuration" => {
                let items = message.pointer("/params/items").and_then(Value::as_array).context("配置请求格式无效")?;
                if items.len() > 32 { bail!("配置请求项超过上限") }
                Value::Array(items.iter().map(|item| snapshot.configuration(item.get("section").and_then(Value::as_str).unwrap_or(""))).collect())
            }
            "workspace/workspaceFolders" => json!([{"uri": file_uri(&snapshot.workspace)?, "name": "CodeTrail captured source"}]),
            "window/workDoneProgress/create" | "client/registerCapability" | "client/unregisterCapability" => Value::Null,
            "workspace/applyEdit" => json!({"applied": false, "failureReason": "CodeTrail is a read-only client"}),
            _ => return self.send(json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": "Unsupported read-only client request"}})),
        };
        self.send(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.stdin.take();
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }
}

fn read_message(reader: &mut impl BufRead, total: &mut usize) -> Result<Value> {
    let mut header_bytes = 0;
    let mut length = None;
    loop {
        let mut line = Vec::new();
        let count = reader
            .take((MAX_HEADER_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)?;
        if count == 0 {
            bail!("Pyright 输出已关闭")
        }
        header_bytes += count;
        if header_bytes > MAX_HEADER_BYTES {
            bail!("LSP 消息头超过上限")
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        let text = std::str::from_utf8(&line)
            .context("LSP 消息头不是 UTF-8")?
            .trim();
        let (name, value) = text.split_once(':').context("无效 LSP 消息头")?;
        if name.eq_ignore_ascii_case("Content-Length") {
            if length.is_some() {
                bail!("重复 LSP Content-Length")
            }
            length = Some(
                value
                    .trim()
                    .parse::<usize>()
                    .context("LSP Content-Length 无效")?,
            );
        }
    }
    let length = length.context("LSP 消息缺少 Content-Length")?;
    if length > MAX_MESSAGE_BYTES {
        bail!("LSP 单条消息超过 2 MiB 上限")
    }
    *total = total
        .checked_add(header_bytes + length)
        .context("LSP 输出长度溢出")?;
    if *total > MAX_OUTPUT_BYTES {
        bail!("LSP 总输出超过 8 MiB 上限")
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body).context("LSP 消息不是有效 JSON")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: u32,
    character: u32,
}

impl Position {
    fn from_value(value: &Value) -> Option<Self> {
        Some(Self {
            line: value.get("line")?.as_u64()?.try_into().ok()?,
            character: value.get("character")?.as_u64()?.try_into().ok()?,
        })
    }

    fn json(self) -> Value {
        json!({"line": self.line, "character": self.character})
    }
}

#[derive(Debug)]
struct Location {
    uri: String,
    start: Position,
    end: Position,
}

fn locations(value: &Value) -> Result<Vec<Location>> {
    if value.is_null() {
        return Ok(Vec::new());
    }
    let values: Vec<&Value> = if let Some(array) = value.as_array() {
        array.iter().collect()
    } else {
        vec![value]
    };
    if values.len() > 10_000 {
        bail!("LSP 位置结果超过上限")
    }
    values
        .into_iter()
        .map(|value| {
            let uri = value
                .get("uri")
                .or_else(|| value.get("targetUri"))
                .and_then(Value::as_str)
                .context("LSP 位置缺少 URI")?;
            let range = value
                .get("targetSelectionRange")
                .or_else(|| value.get("range"))
                .context("LSP 位置缺少范围")?;
            let start = range
                .get("start")
                .and_then(Position::from_value)
                .context("LSP 起始位置无效")?;
            let end = range
                .get("end")
                .and_then(Position::from_value)
                .context("LSP 结束位置无效")?;
            if end < start {
                bail!("LSP 范围顺序无效")
            }
            Ok(Location {
                uri: uri.to_owned(),
                start,
                end,
            })
        })
        .collect()
}

struct Declaration {
    symbol_index: usize,
    file: String,
    start: Position,
    end: Position,
}

type DeclarationIndex = BTreeMap<(String, u32, u32, String), Vec<usize>>;

fn declarations(snapshot: &Snapshot, symbols: &[Symbol]) -> Result<Vec<Declaration>> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let mut declarations = Vec::new();
    let mut index: DeclarationIndex = BTreeMap::new();
    for (position, symbol) in symbols.iter().enumerate() {
        index
            .entry((
                symbol.range.file.clone(),
                symbol.range.start_line,
                symbol.range.end_line,
                symbol.name.clone(),
            ))
            .or_default()
            .push(position);
    }
    for (file, text) in &snapshot.files {
        let tree = parser.parse(text, None).context("Python 声明解析失败")?;
        visit_declarations(tree.root_node(), file, text, &index, &mut declarations, 0);
    }
    Ok(declarations)
}

fn visit_declarations(
    node: Node<'_>,
    file: &str,
    text: &str,
    index: &DeclarationIndex,
    output: &mut Vec<Declaration>,
    depth: usize,
) {
    if depth > 256 {
        return;
    }
    if matches!(node.kind(), "function_definition" | "class_definition") {
        if let Some(name) = node.child_by_field_name("name") {
            let label = text.get(name.byte_range()).unwrap_or("");
            let start_line = node
                .parent()
                .filter(|parent| parent.kind() == "decorated_definition")
                .unwrap_or(node)
                .start_position()
                .row as u32
                + 1;
            let end_line =
                node.end_position().row as u32 + u32::from(node.end_position().column > 0);
            let matched = index.get(&(file.to_owned(), start_line, end_line, label.to_owned()));
            if let Some([symbol_index]) = matched.map(Vec::as_slice) {
                let point = name.start_position();
                if let Some(line) = text.lines().nth(point.row) {
                    if let Some(prefix) = line.get(..point.column) {
                        let start = Position {
                            line: point.row as u32,
                            character: prefix.encode_utf16().count() as u32,
                        };
                        let end = Position {
                            line: start.line,
                            character: start.character + label.encode_utf16().count() as u32,
                        };
                        output.push(Declaration {
                            symbol_index: *symbol_index,
                            file: file.to_owned(),
                            start,
                            end,
                        });
                    }
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_declarations(child, file, text, index, output, depth + 1);
    }
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct CallKey {
    file: String,
    start_line: u32,
    end_line: u32,
    column: u32,
    text: String,
}

type CallTokens = BTreeMap<CallKey, Vec<(usize, usize)>>;

/// Go-to-definition at `factory` inside `factory()()` identifies the factory,
/// never the object returned by it. A location alone cannot distinguish those
/// two AST calls, so require the same full token identity used by relations.rs.
fn eligible_sites(snapshot: &Snapshot, relations: &[CodeRelation]) -> Result<Vec<bool>> {
    let mut parser = Parser::new();
    parser.set_language(&tree_sitter_python::LANGUAGE.into())?;
    let mut tokens = CallTokens::new();
    for (file, text) in &snapshot.files {
        let tree = parser.parse(text, None).context("Python 调用点解析失败")?;
        visit_call_tokens(tree.root_node(), file, text, &mut tokens, 0);
    }
    Ok(relations
        .iter()
        .map(|relation| {
            if !snapshot.files.contains_key(&relation.site.file) {
                return false;
            }
            if relation.kind != RelationKind::Call {
                return true;
            }
            let key = CallKey {
                file: relation.site.file.clone(),
                start_line: relation.site.start_line,
                end_line: relation.site.end_line,
                column: relation.site_column,
                text: relation.target_name.clone(),
            };
            tokens.get(&key).is_some_and(|ranges| {
                ranges.iter().any(|(start, end)| {
                    let identity = format!(
                        "{}\0{start}\0{end}\0call\0{}\0{}",
                        relation.site.file,
                        relation.source_symbol_id.as_deref().unwrap_or(""),
                        relation.target_name
                    );
                    relation.id == format!("rel:{}", crate::digest(identity.as_bytes()))
                })
            })
        })
        .collect())
}

fn visit_call_tokens(
    node: Node<'_>,
    file: &str,
    text: &str,
    tokens: &mut CallTokens,
    depth: usize,
) {
    if depth > 256 {
        return;
    }
    if node.kind() == "call" {
        if let Some(function) = node.child_by_field_name("function") {
            if let Some(token) = direct_callee_token(function) {
                let point = token.start_position();
                let line_start = token.start_byte().saturating_sub(point.column);
                if let Some(prefix) = text.get(line_start..token.start_byte()) {
                    let key = CallKey {
                        file: file.to_owned(),
                        start_line: point.row as u32 + 1,
                        end_line: token.end_position().row as u32
                            + u32::from(token.end_position().column > 0),
                        column: prefix.encode_utf16().count() as u32 + 1,
                        text: text
                            .get(function.byte_range())
                            .unwrap_or("")
                            .chars()
                            .take(180)
                            .collect(),
                    };
                    tokens
                        .entry(key)
                        .or_default()
                        .push((token.start_byte(), function.end_byte()));
                }
            }
        }
    }
    let mut cursor = node.walk();
    for child in node.named_children(&mut cursor) {
        visit_call_tokens(child, file, text, tokens, depth + 1);
    }
}

fn direct_callee_token(mut node: Node<'_>) -> Option<Node<'_>> {
    for _ in 0..256 {
        match node.kind() {
            "identifier" => return Some(node),
            "attribute" => {
                return node
                    .child_by_field_name("attribute")
                    .filter(|attribute| attribute.kind() == "identifier")
            }
            "parenthesized_expression" if node.named_child_count() == 1 => {
                node = node.named_child(0)?
            }
            _ => return None,
        }
    }
    None
}

fn bind_relation(relation: &mut CodeRelation, symbol: &Symbol, method: &str) {
    relation.target_symbol_id = Some(symbol.id.clone());
    relation.target = Some(symbol.range.clone());
    relation.target_name = symbol.qualified_name.clone();
    relation.resolution = RelationResolution::Resolved;
    relation.origin = RelationOrigin::Lsp;
    relation.detail = format!(
        "Pyright {method} 返回的精确源码位置已匹配声明；静态语义绑定不代表运行时一定调用。"
    );
}

fn resolve_definition(
    relation: &mut CodeRelation,
    result: &[Location],
    snapshot: &Snapshot,
    declarations: &[Declaration],
    symbols: &[Symbol],
) {
    if result.is_empty() {
        return;
    }
    let mut candidates = BTreeSet::new();
    let mut outside = false;
    let mut non_declaration = false;
    for location in result {
        let Some(file) = snapshot.relative_uri(&location.uri) else {
            outside = true;
            continue;
        };
        let matching: Vec<_> = declarations
            .iter()
            .filter(|declaration| {
                declaration.file == file
                    && (location.start == declaration.start
                        || (location.start <= declaration.start && declaration.end <= location.end))
            })
            .collect();
        if matching.is_empty() {
            non_declaration = true;
        }
        candidates.extend(
            matching
                .into_iter()
                .map(|declaration| declaration.symbol_index),
        );
    }
    if candidates.len() == 1 && !outside && !non_declaration {
        bind_relation(
            relation,
            &symbols[*candidates.first().expect("one candidate")],
            "definition",
        );
        return;
    }
    relation.target_symbol_id = None;
    relation.target = None;
    relation.origin = RelationOrigin::Lsp;
    if candidates.len() > 1 || (!candidates.is_empty() && (outside || non_declaration)) {
        relation.resolution = RelationResolution::Ambiguous;
        relation.detail = "Pyright 返回多个可能的定义位置，未建立唯一目标关系。".into();
    } else if outside && !non_declaration {
        relation.resolution = RelationResolution::External;
        relation.detail =
            "Pyright 定义在捕获项目之外（例如标准库或依赖），未建立仓库内调用边。".into();
    } else {
        relation.resolution = RelationResolution::Unresolved;
        relation.detail =
            "Pyright 指向参数或其他未索引声明；不能将包含它的函数当作调用目标。".into();
    }
}

struct EnrichmentInputs<'a> {
    files: &'a BTreeMap<String, String>,
    symbols: &'a [Symbol],
    relations: &'a mut [CodeRelation],
    focus: &'a Symbol,
    blocked_relation_ids: &'a BTreeSet<String>,
}

#[cfg(test)]
fn enrich_using(
    files: &BTreeMap<String, String>,
    symbols: &[Symbol],
    relations: &mut [CodeRelation],
    focus: &Symbol,
    command: ServerCommand,
    limits: Limits,
    started: Instant,
) -> Result<SemanticReport> {
    enrich_using_policy(
        EnrichmentInputs {
            files,
            symbols,
            relations,
            focus,
            blocked_relation_ids: &BTreeSet::new(),
        },
        command,
        limits,
        started,
    )
}

fn enrich_using_policy(
    input: EnrichmentInputs<'_>,
    command: ServerCommand,
    limits: Limits,
    started: Instant,
) -> Result<SemanticReport> {
    let EnrichmentInputs {
        files,
        symbols,
        relations,
        focus,
        blocked_relation_ids,
    } = input;
    let snapshot = Snapshot::new(files)?;
    let declarations = declarations(&snapshot, symbols)?;
    let mut eligible = eligible_sites(&snapshot, relations)?;
    for (index, relation) in relations.iter().enumerate() {
        if blocked_relation_ids.contains(&relation.id) {
            eligible[index] = false;
        }
    }
    let focus_declaration = declarations
        .iter()
        .find(|declaration| symbols[declaration.symbol_index].id == focus.id)
        .context("选中符号没有可核对的 Python 声明位置")?;
    let mut client = LspClient::start(command.command(&snapshot)?, started + limits.total)?;
    let workspace_uri = file_uri(&snapshot.workspace)?;
    let initialize = client.request(
        "initialize",
        json!({
            "processId": null,
            "clientInfo": {"name": "CodeTrail read-only snapshot", "version": "0.2"},
            "rootUri": workspace_uri,
            "rootPath": snapshot.workspace,
            "workspaceFolders": [{"uri": workspace_uri, "name": "CodeTrail captured source"}],
            "capabilities": {
                "workspace": {"configuration": true, "workspaceFolders": true, "applyEdit": false},
                "textDocument": {"definition": {"linkSupport": true}, "references": {}},
                "general": {"positionEncodings": ["utf-16"]}
            },
            "initializationOptions": {"disablePullDiagnostics": true}
        }),
        limits.initialize,
        &snapshot,
    )?;
    let capability = |key: &str| {
        initialize
            .get("capabilities")
            .and_then(|caps| caps.get(key))
            .is_some_and(|value| value != &Value::Bool(false) && !value.is_null())
    };
    if !capability("definitionProvider") {
        bail!("语言服务器未声明 definition 支持")
    }
    if initialize
        .pointer("/capabilities/positionEncoding")
        .and_then(Value::as_str)
        .is_some_and(|encoding| encoding != "utf-16")
    {
        bail!("语言服务器未使用 UTF-16 位置编码")
    }
    let supports_references = capability("referencesProvider");
    client.notify("initialized", json!({}))?;
    for (path, text) in &snapshot.files {
        client.notify(
            "textDocument/didOpen",
            json!({"textDocument": {
                "uri": snapshot.uris[path], "languageId": "python", "version": 1, "text": text
            }}),
        )?;
    }
    let mut staged = relations.to_vec();
    let mut warnings = Vec::new();
    let mut partial = false;
    let mut checked = 0;
    let mut order: Vec<_> = staged
        .iter()
        .enumerate()
        .filter(|(index, _)| eligible[*index])
        .map(|(index, _)| index)
        .collect();
    let skipped_calls = staged
        .iter()
        .enumerate()
        .filter(|(index, relation)| {
            relation.kind == RelationKind::Call
                && snapshot.files.contains_key(&relation.site.file)
                && !eligible[*index]
        })
        .count();
    if skipped_calls > 0 {
        partial = true;
        warnings.push(format!("{skipped_calls} 个动态、已知改写或无法精确匹配的调用点保留语法判断；不把返回值、下标或条件表达式的内部名字定义当作实际调用目标。"));
    }
    let blocked_count = staged
        .iter()
        .filter(|relation| blocked_relation_ids.contains(&relation.id))
        .count();
    if blocked_count > 0 {
        partial = true;
        warnings.push(format!("静态分析已观察到 {blocked_count} 处绑定失效；LSP 声明/引用位置不能消除这些动态改写证据。"));
    }
    // When capped, prefer focus outgoing and known incoming sites first.
    order.sort_by_key(|index| {
        let relation = &staged[*index];
        let focused = relation.source_symbol_id.as_deref() == Some(&focus.id)
            || relation.target_symbol_id.as_deref() == Some(&focus.id);
        (
            !focused,
            relation.resolution == RelationResolution::Resolved,
            *index,
        )
    });
    if order.len() > limits.definitions {
        partial = true;
        warnings.push(format!(
            "Pyright 本次最多核对 {} 个 AST 位置；其余位置保留语法结果。",
            limits.definitions
        ));
    }
    for index in order.into_iter().take(limits.definitions) {
        let relation = &mut staged[index];
        let line = relation
            .site
            .start_line
            .checked_sub(1)
            .context("AST 行号必须从 1 开始")?;
        let character = relation
            .site_column
            .checked_sub(1)
            .context("AST UTF-16 列号必须从 1 开始")?;
        let text_line = snapshot.files[&relation.site.file]
            .lines()
            .nth(line as usize)
            .context("AST 位置不在捕获文件中")?;
        if character as usize > text_line.encode_utf16().count() {
            bail!("AST UTF-16 列号超出捕获行")
        }
        let result = client.request(
            "textDocument/definition",
            json!({
                "textDocument": {"uri": snapshot.uris[&relation.site.file]},
                "position": Position { line, character }.json()
            }),
            limits.request,
            &snapshot,
        )?;
        resolve_definition(
            relation,
            &locations(&result)?,
            &snapshot,
            &declarations,
            symbols,
        );
        checked += 1;
    }
    let mut matched_references = 0;
    let mut returned_references = 0;
    if supports_references {
        let result = client.request(
            "textDocument/references",
            json!({
                "textDocument": {"uri": snapshot.uris[&focus_declaration.file]},
                "position": focus_declaration.start.json(),
                "context": {"includeDeclaration": false}
            }),
            limits.request,
            &snapshot,
        )?;
        for location in locations(&result)? {
            returned_references += 1;
            let Some(file) = snapshot.relative_uri(&location.uri) else {
                continue;
            };
            for (index, relation) in staged.iter_mut().enumerate() {
                if !eligible[index] {
                    continue;
                }
                let site = Position {
                    line: relation.site.start_line.saturating_sub(1),
                    character: relation.site_column.saturating_sub(1),
                };
                if relation.site.file != file || !(location.start <= site && site < location.end) {
                    continue;
                }
                // A reference result shows that this target is possible, not
                // that it is the only one. Do not erase a definition ambiguity
                // (or a contradictory non-declaration/external result).
                if relation.origin == RelationOrigin::Lsp
                    && relation.target_symbol_id.as_ref() != Some(&focus.id)
                {
                    relation.target_symbol_id = None;
                    relation.target = None;
                    relation.resolution = RelationResolution::Ambiguous;
                    relation.detail =
                        "Pyright definition 与 references 返回不同目标，保留为歧义。".into();
                    partial = true;
                } else {
                    bind_relation(relation, focus, "references");
                }
                matched_references += 1;
            }
        }
    } else {
        partial = true;
        warnings.push(
            "语言服务器未声明 references 支持；已保留真实 AST 调用点的 definition 结果。".into(),
        );
    }
    // A bounded shutdown is courteous; Drop also kills/reaps every process and
    // joins the readers even when a server ignores protocol shutdown.
    let _ = client.request(
        "shutdown",
        Value::Null,
        Duration::from_millis(200),
        &snapshot,
    );
    let _ = client.notify("exit", Value::Null);
    if !client.warnings.is_empty() {
        partial = true;
    }
    warnings.append(&mut client.warnings);
    for relation in &mut staged {
        if relation.origin != RelationOrigin::Lsp
            || relation.resolution != RelationResolution::Resolved
        {
            continue;
        }
        let Some(target) = &relation.target else {
            continue;
        };
        if snapshot
            .files
            .get(&target.file)
            .and_then(|text| {
                text.lines()
                    .nth(target.start_line.saturating_sub(1) as usize)
            })
            .is_some_and(|line| line.trim_start().starts_with('@'))
        {
            relation
                .detail
                .push_str(" 该定义含装饰器；声明位置不能证明装饰后的运行时可调用对象。");
        }
    }
    relations.clone_from_slice(&staged);
    Ok(SemanticReport {
        engine: RelationEngine {
            name: "Python AST + Pyright LSP".into(),
            status: if partial { RelationEngineStatus::Partial } else { RelationEngineStatus::Ready },
            detail: format!("已核对 {checked} 个 AST 位置；references 返回 {returned_references} 处，其中 {matched_references} 处匹配现有 AST 位置。使用独立 Python 3.12 源码快照，不读取项目配置、虚拟环境或运行项目；动态调用与外部依赖可能仍未知。"),
        },
        warnings,
    })
}

#[cfg(test)]
#[path = "semantic_tests.rs"]
mod tests;
