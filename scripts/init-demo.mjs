import { existsSync, mkdirSync, readFileSync, writeFileSync, copyFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { spawnSync } from 'node:child_process';

const project = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const target = resolve(process.argv[2] ?? resolve(project, 'demo-workspace'));
if (existsSync(target)) throw new Error(`目标已存在，为保护现有文件不覆盖：${target}\n请指定新的目录：npm run demo:init -- /path/to/new-demo`);
const build = spawnSync('cargo', ['build', '-p', 'reader-cli'], { cwd: project, stdio: 'inherit' });
if (build.status !== 0) throw new Error('无法构建 reader-cli，请先运行 npm run doctor');
mkdirSync(target, { recursive: true });
const files = { 'agent/model_router.py': 'demo/baseline/model_router.py', 'agent/tool_runner.py': 'demo/agent/tool_runner.py', 'agent/chat_entry.py': 'demo/agent/chat_entry.py', 'web/chat_state.ts': 'demo/agent/chat_state.ts', 'tests/test_router.py': 'demo/agent/test_router.py', 'README.md': 'demo/agent/README.md' };
for (const [destination, source] of Object.entries(files)) {
  const path = resolve(target, destination); mkdirSync(dirname(path), { recursive: true }); copyFileSync(resolve(project, source), path);
}
writeFileSync(resolve(target, '.gitignore'), '.reader/\n__pycache__/\n');
function git(...args) {
  const result = spawnSync('git', ['-c', 'core.hooksPath=/dev/null', ...args], { cwd: target, encoding: 'utf8' });
  if (result.status !== 0) throw new Error(result.stderr || 'Git 初始化失败');
}
git('init'); git('add', '.'); git('-c', 'user.name=CodeTrail Demo', '-c', 'user.email=demo@codetrail.invalid', '-c', 'commit.gpgsign=false', 'commit', '-m', 'Demo baseline');
const binary = resolve(project, 'target/debug', process.platform === 'win32' ? 'reader-cli.exe' : 'reader-cli');
function rpc(command, payload = {}) {
  const result = spawnSync(binary, ['rpc', target], { input: JSON.stringify({ command, payload }) + '\n', encoding: 'utf8', timeout: 30000 });
  if (result.error) throw result.error;
  const response = JSON.parse(result.stdout);
  if (!response.ok) throw new Error(response.error);
  return response.value;
}
const groups = [
  ['模型路由', '#a78bfa', '按复杂度选择本地或云端模型；关注阈值与输入范围。', ['choose_route', 'test_complex_tasks_use_cloud']],
  ['流式输出与降级', '#38bdf8', '输出第一个 token 后不自动切换模型，避免重复输出。', ['stream_with_fallback', 'appendToken', 'failReply']],
  ['上下文管理', '#34d399', '保留系统约束，裁剪历史消息。关联测试不等于覆盖率。', ['build_context', 'test_system_message_is_preserved']],
  ['工具调用', '#fbbf24', '校验请求、确认授权，然后执行工具。', ['validate_request', 'execute_tool']],
  ['对话入口', '#fb923c', '入口自身没有修改，但依赖的路由与降级行为变化后需要检查。', ['stream_chat']],
  ['路由数据结构', '#94a3b8', '路由配置结构未修改；调用它的函数变化不代表这个结构本身受影响。', ['Route']],
];
for (const [name, color, description, names] of groups) {
  let snapshot = rpc('saveFeature', { name, color, description });
  const feature = snapshot.features.find(f => f.name === name);
  for (const symbolName of names) {
    const symbol = snapshot.symbols.find(s => s.name === symbolName);
    if (!symbol) throw new Error(`示例符号解析失败：${symbolName}`);
    snapshot = rpc('bindAnchor', { featureId: feature.id, path: symbol.range.file, startLine: symbol.range.startLine, endLine: symbol.range.endLine, symbolId: symbol.id, note: '' });
  }
  rpc('setReview', { featureId: feature.id, state: name === '工具调用' ? 'reading' : 'verified', expectedDigest: snapshot.features.find(f => f.id === feature.id).currentDigest });
}
copyFileSync(resolve(project, 'demo/agent/model_router.py'), resolve(target, 'agent/model_router.py'));
writeFileSync(resolve(target, 'agent/review_notes.md'), '# 待审阅\n\n检查路由阈值和流式降级的改动。此文件用于演示未跟踪文件。\n');
const result = rpc('openProject');
console.log(`\n真实 Git 示例已创建：${target}\n在 CodeTrail 桌面窗口点“打开仓库”并选择此目录。\n`);
console.log(result.features.map(f => `${f.name}: ${f.reviewState}`).join('\n'));

