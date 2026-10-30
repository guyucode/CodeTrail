// Linux WebDriver smoke test for the compiled Tauri application.
// Start tauri-driver first; see docs/VALIDATION.md. Never tests against user code.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const project = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const application = resolve(process.env.TAURI_APPLICATION ?? join(project, 'src-tauri/target/debug/codetrail-desktop'));
const driver = (process.env.WEBDRIVER_URL ?? 'http://127.0.0.1:4444').replace(/\/$/, '');
assert.ok(existsSync(application), `Build the desktop application first: ${application}`);
const temporary = mkdtempSync(join(tmpdir(), 'codetrail-native-'));
const repository = join(temporary, 'repository');
let session;
let assertions = 0;
function check(condition, message) { assert.ok(condition, message); assertions++; }

async function request(method, path, body) {
  const response = await fetch(`${driver}${path}`, {
    method, headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    signal: AbortSignal.timeout(60000),
  });
  const result = await response.json();
  if (!response.ok || (result.value?.error && result.value?.ok !== false)) {
    throw new Error(result.value?.message || JSON.stringify(result));
  }
  return result.value;
}
async function evaluate(script, args = [], async = false) {
  return request('POST', `/session/${session}/execute/${async ? 'async' : 'sync'}`, { script, args });
}
async function waitFor(script) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    if (await evaluate(script)) return true;
    await new Promise(resolve => setTimeout(resolve, 250));
  }
  return false;
}
async function clickTab(label) {
  // A WebDriver input event supplies real user activation. Calling el.click()
  // from injected JavaScript would trigger WebKit's clipboard workaround with
  // an untrusted event and produce artificial canceled clipboard promises.
  const element = await request('POST', `/session/${session}/element`, {
    using: 'xpath', value: `//*[@role='tab'][normalize-space(.)='${label}']`,
  });
  const id = element['element-6066-11e4-a52e-4f735466cecf'];
  return request('POST', `/session/${session}/element/${id}/click`, {});
}
async function clickXPath(value) {
  const element = await request('POST', `/session/${session}/element`, { using: 'xpath', value });
  return request('POST', `/session/${session}/element/${element['element-6066-11e4-a52e-4f735466cecf']}/click`, {});
}
function x11(...args) {
  const result = spawnSync('xdotool', args, { encoding: 'utf8', timeout: 5000 });
  if (result.error || result.status !== 0) throw result.error ?? new Error(result.stderr || 'xdotool failed');
  return result.stdout.trim();
}
async function selectRepository() {
  assert.ok(process.env.DISPLAY, 'Native folder-dialog tests require DISPLAY; use test:native:managed under xvfb-run.');
  let dialog;
  const deadline = Date.now() + 10000;
  while (Date.now() < deadline) {
    const result = spawnSync('xdotool', ['search', '--onlyvisible', '--name', '选择要阅读的代码仓库'], { encoding: 'utf8', timeout: 2000 });
    if (result.status === 0 && result.stdout.trim()) { dialog = result.stdout.trim().split('\n')[0]; break; }
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  assert.ok(dialog, 'The actual native directory chooser should be visible');
  x11('windowfocus', '--sync', dialog);
  // Leave GTK's empty Recent view so directory selection is enabled.
  x11('key', '--clearmodifiers', 'alt+Home');
  await new Promise(resolve => setTimeout(resolve, 300));
  x11('key', '--clearmodifiers', 'ctrl+l');
  x11('key', '--clearmodifiers', 'ctrl+a');
  x11('type', '--clearmodifiers', '--delay', '1', repository);
  // GTK resolves the location asynchronously. Enter before its completion
  // popup settles can be consumed without selecting or entering the folder.
  await new Promise(resolve => setTimeout(resolve, 500));
  x11('key', '--clearmodifiers', 'Return');
  await new Promise(resolve => setTimeout(resolve, 500));
  const geometry = Object.fromEntries(x11('getwindowgeometry', '--shell', dialog).split('\n').map(line => line.split('=')));
  // The completion popup can consume the first pointer click. Click GTK's
  // bottom-right Open button again only if the same dialog remains visible.
  for (let attempt = 0; attempt < 3; attempt++) {
    const visible = spawnSync('xdotool', ['search', '--onlyvisible', '--name', '选择要阅读的代码仓库'], { encoding: 'utf8', timeout: 2000 });
    if (visible.status !== 0 || !visible.stdout.trim().split('\n').includes(dialog)) break;
    x11('mousemove', '--window', dialog, String(Number(geometry.WIDTH) - 48), String(Number(geometry.HEIGHT) - 24), 'click', '1');
    await new Promise(resolve => setTimeout(resolve, 500));
  }
}
async function invoke(command, payload = {}, expectError = false) {
  const result = await evaluate(`
    const command = arguments[0], payload = arguments[1], done = arguments[arguments.length - 1];
    window.__TAURI_INTERNALS__.invoke(command, payload)
      .then(value => done({ok: true, value}))
      .catch(error => done({ok: false, error: String(error)}));
  `, [command, payload], true);
  if (expectError) { check(!result.ok, `${command} should reject this request`); return result.error; }
  if (!result.ok) throw new Error(`${command}: ${result.error}`);
  return result.value;
}
function sourceHash() {
  const hash = createHash('sha256');
  function visit(directory, prefix = '') {
    for (const entry of readdirSync(directory, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
      if (['.git', '.reader'].includes(entry.name)) continue;
      const relative = `${prefix}${entry.name}`;
      if (entry.isDirectory()) visit(join(directory, entry.name), `${relative}/`);
      else hash.update(relative).update('\0').update(readFileSync(join(directory, entry.name))).update('\0');
    }
  }
  visit(repository); return hash.digest('hex');
}

try {
  const initialized = spawnSync(process.execPath, ['scripts/init-demo.mjs', repository], {
    cwd: project, stdio: 'inherit', timeout: 180000,
  });
  assert.equal(initialized.status, 0, 'Could not initialize the isolated fixture');
  const originalHash = sourceHash();
  const created = await request('POST', '/session', {
    capabilities: { alwaysMatch: { 'tauri:options': { application } } },
  });
  session = created.sessionId;
  await request('POST', `/session/${session}/timeouts`, { script: 50000 });
  const ready = await waitFor('return !!window.__TAURI_INTERNALS__ && !!document.querySelector(".monaco-editor");');
  check(ready, 'Native WebView must load the bundled Monaco editor');
  check((await evaluate('return document.body.innerText;')).includes('CodeTrail'), 'Native UI should render');
  await evaluate(`
    window.__smokeErrors = [];
    window.addEventListener('error', event => window.__smokeErrors.push({kind: 'error', message: String(event.message), stack: String(event.error?.stack), stage: window.__smokeStage}));
    window.addEventListener('unhandledrejection', event => window.__smokeErrors.push({kind: 'rejection', message: String(event.reason), stack: String(event.reason?.stack), stage: window.__smokeStage}));
  `);
  for (let iteration = 0; iteration < 2; iteration++) {
    await evaluate(`window.__smokeStage = 'open diff';`);
    await clickTab('本次改动');
    check(await waitFor('return !!document.querySelector(".monaco-diff-editor .line-insert");'), 'Native Monaco worker computes the diff');
    await evaluate(`window.__smokeStage = 'return source';`);
    await clickTab('完整代码');
    check(await waitFor('return !!document.querySelector(".editor-container .monaco-editor .view-lines") && !document.querySelector(".monaco-diff-editor");'), 'Native source view survives diff model disposal');
  }
  const nativeErrors = await evaluate('return window.__smokeErrors;');
  check(nativeErrors.length === 0, `Native view transitions produce no JavaScript errors: ${JSON.stringify(nativeErrors)}`);

  // The native dialog is operated through X11 input; the protected Tauri bridge
  // is not modified. All subsequent UI operations use the real Rust session.
  await clickXPath("//button[normalize-space(.)='打开仓库']");
  await selectRepository();
  check(await waitFor('return !document.querySelector(".demo-tag") && document.body.innerText.includes("repository");'), 'Real project opens through the desktop UI and Tauri API');
  await clickXPath("//summary[normalize-space(.)='更多']");
  await clickXPath("//button[normalize-space(.)='相关逻辑']");
  await evaluate(`
    const picker = document.querySelector('[aria-label="分析关系的符号"]');
    picker.value = 'agent/model_router.py::choose_route';
    picker.dispatchEvent(new Event('change', {bubbles: true}));
  `);
  check(await waitFor('return document.querySelector(".incoming-relations")?.innerText.includes("stream_chat");'), 'Native UI shows real cross-file callers');
  check((await evaluate('return document.querySelector(".reference-relations")?.innerText || "";')).includes('get_route_callback'), 'Native UI separates function-value references');
  check((await evaluate('return document.querySelector(".relation-impacts")?.innerText || "";')).includes('可能受影响'), 'Native UI shows possible dependency impact');
  const entryPath = join(repository, 'agent/chat_entry.py');
  const originalEntry = readFileSync(entryPath, 'utf8');
  try {
    writeFileSync(entryPath, '# Changed externally after analysis\n' + originalEntry);
    await clickXPath("//button[starts-with(@aria-label, '查看调用位置：stream_chat →')]");
    check(await waitFor('return document.querySelector(".relation-navigation-error")?.innerText.includes("已在分析后变化");'), 'Native evidence navigation rejects stale source positions');
    check(!!(await evaluate('return !!document.querySelector(".relations-view");')), 'Stale evidence does not silently navigate to source');
  } finally { writeFileSync(entryPath, originalEntry); }
  await clickXPath("//button[@aria-label='刷新关系']");
  check(await waitFor('return !!document.querySelector(".incoming-relations") && !document.querySelector(".relation-navigation-error");'), 'Refreshing restores a coherent evidence snapshot');

  let snapshot = await invoke('open_project', { path: repository });
  check(snapshot.git.available && snapshot.symbols.length >= 12, 'Real Rust parser and Git should be available');
  check(snapshot.features.find(f => f.name === '模型路由')?.reviewState === 'needsReview', 'Changed baseline feature requires review');
  check(snapshot.features.find(f => f.name === '上下文管理')?.reviewState === 'verified', 'Unchanged baseline feature remains verified');
  const file = await invoke('read_file', { path: 'agent/model_router.py' });
  check(file.content.includes('complexity < 0.65') && file.hash.length === 64, 'IPC reads real source content with its hash');
  const changes = await invoke('get_changes', { base: 'HEAD', head: 'WORKTREE' });
  const router = changes.files.find(f => f.path === 'agent/model_router.py');
  check(router?.additions === 9 && router?.deletions === 4, 'Git line totals should match the fixture');
  check(router.featureIds.length >= 2, 'Changes map to directly affected features');
  check(changes.files.some(f => f.status === 'untracked'), 'Untracked files are visible');
  const diff = await invoke('get_file_diff', { path: 'agent/model_router.py', base: 'HEAD', head: 'WORKTREE' });
  check(diff.oldContent.includes('complexity < 0.5') && diff.newContent === file.content, 'Native diff reads both revisions');
  const relations = await invoke('get_relations', { symbolId: 'agent/model_router.py::choose_route', base: 'HEAD', head: 'WORKTREE', semantic: false });
  check(relations.incoming.some(e => e.sourceName === 'stream_chat' && e.origin === 'syntax'), 'Static relation API resolves the imported alias');
  check(relations.references.some(e => e.sourceName === 'get_route_callback' && e.kind === 'reference'), 'Reference API does not turn a returned function into a call');
  const entryFeature = snapshot.features.find(f => f.name === '对话入口');
  const schemaFeature = snapshot.features.find(f => f.name === '路由数据结构');
  check(relations.impacts.some(i => i.featureId === entryFeature.id && i.kind === 'possible') && entryFeature.reviewState === 'verified', 'Potential impact preserves prior human review state');
  check(!relations.impacts.some(i => i.featureId === schemaFeature.id), 'Changed callers do not automatically mark their unchanged callees as impacted');
  check(relations.fileHashes[file.path] === file.hash && relations.sourceDigest.length === 64, 'Relationship evidence is tied to captured source hashes');
  const semantic = await invoke('get_relations', { symbolId: 'agent/model_router.py::choose_route', base: 'HEAD', head: 'WORKTREE', semantic: true });
  check([...semantic.incoming, ...semantic.references].some(e => e.origin === 'lsp'), `Actual Pyright returns semantic evidence: ${semantic.engine.detail}`);
  const historical = await invoke('get_relations', { symbolId: 'agent/model_router.py::choose_route', base: 'HEAD', head: 'HEAD', semantic: false });
  check(historical.impacts.length === 0 && historical.changedSymbolIds.length === 0 && historical.warnings.some(w => w.includes('历史')), 'Current graph never masquerades as historical impact');

  snapshot = await invoke('save_feature', { draft: { name: '原生联调功能', description: '由测试创建', color: '#a78bfa' } });
  let feature = snapshot.features.find(f => f.name === '原生联调功能');
  const symbol = snapshot.symbols.find(s => s.name === 'choose_route');
  check(!!feature && !!symbol, 'Feature creation and symbol discovery should work');
  snapshot = await invoke('bind_anchor', { request: {
    featureId: feature.id, path: symbol.range.file, startLine: symbol.range.startLine,
    endLine: symbol.range.endLine, symbolId: symbol.id, note: '核对模型路由阈值',
  } });
  feature = snapshot.features.find(f => f.id === feature.id);
  check(feature.anchors.length === 1, 'Symbol binding persists');
  snapshot = await invoke('set_review', { featureId: feature.id, state: 'verified', expectedDigest: feature.currentDigest });
  feature = snapshot.features.find(f => f.id === feature.id);
  check(feature.reviewState === 'verified', 'Current digest can be confirmed through IPC');
  const staleDigest = feature.currentDigest;
  const sourcePath = join(repository, 'agent/model_router.py');
  const original = readFileSync(sourcePath, 'utf8');
  try {
    // This explicit test edit simulates an external coding agent. The reader
    // commands themselves must never write source files.
    writeFileSync(sourcePath, original.replace('complexity < 0.65', 'complexity < 0.70'));
    await invoke('set_review', { featureId: feature.id, state: 'verified', expectedDigest: staleDigest }, true);
    snapshot = await invoke('refresh_project');
    feature = snapshot.features.find(f => f.id === feature.id);
    check(feature.reviewState === 'needsReview' && feature.currentDigest !== staleDigest, 'Refresh invalidates confirmation after code changes');
  } finally { writeFileSync(sourcePath, original); }
  await invoke('read_file', { path: '../outside.py' }, true);
  await invoke('read_file', { path: '.git/config' }, true);
  const report = await invoke('export_review');
  check(report.includes('原生联调功能') && report.includes('agent/model_router.py'), 'Review export contains anchored evidence');
  await invoke('delete_feature', { featureId: feature.id });
  snapshot = await invoke('refresh_project');
  check(!snapshot.features.some(f => f.id === feature.id), 'Feature deletion persists');
  check(sourceHash() === originalHash, 'Reader operations leave every project source byte unchanged');
  const finalErrors = await evaluate('return window.__smokeErrors;');
  check(finalErrors.length === 0, `Native UI remains free of JavaScript errors: ${JSON.stringify(finalErrors)}`);
  console.log(`Native WebView + Rust IPC smoke: ${assertions} assertions passed.`);
} finally {
  if (session) await request('DELETE', `/session/${session}`).catch(error => console.warn(error.message));
  rmSync(temporary, { recursive: true, force: true });
}
