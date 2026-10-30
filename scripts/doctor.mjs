import { spawnSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
const checks = [ ['Node.js', process.execPath, ['--version']], ['npm', process.platform === 'win32' ? 'npm.cmd' : 'npm', ['--version']], ['Rust', 'rustc', ['--version']], ['Cargo', 'cargo', ['--version']], ['Git', 'git', ['--version']] ];
let failed = false;
console.log('\nCodeTrail 开发环境检查\n');
for (const [label, command, args] of checks) {
  const result = spawnSync(command, args, { encoding: 'utf8', shell: process.platform === 'win32' && command.endsWith('.cmd') });
  const ok = result.status === 0;
  if (!ok) failed = true;
  console.log(`${ok ? '✓' : '✗'} ${label}: ${ok ? result.stdout.trim() : '未安装或不在 PATH 中'}`);
}
const [major, minor] = process.versions.node.split('.').map(Number);
if (major < 22 || (major === 22 && minor < 12)) { failed = true; console.log('✗ Node.js 至少需要 22.12；推荐 Node 24 LTS 或更新版本。'); }
if (process.platform === 'darwin') {
  const result = spawnSync('xcode-select', ['-p'], { encoding: 'utf8' });
  console.log(`${result.status === 0 ? '✓' : '✗'} Xcode Command Line Tools: ${result.stdout?.trim() || '请运行 xcode-select --install'}`);
  if (result.status !== 0) failed = true;
}
if (process.platform === 'linux') {
  const result = spawnSync('pkg-config', ['--modversion', 'gtk+-3.0', 'webkit2gtk-4.1'], { encoding: 'utf8' });
  console.log(`${result.status === 0 ? '✓' : '✗'} GTK / WebKit: ${result.stdout?.trim().replaceAll('\n', ' / ') || '缺少 Tauri 原生开发依赖，参阅 README'}`);
  if (result.status !== 0) failed = true;
}
try {
  const metadata = JSON.parse(readFileSync(fileURLToPath(new URL('../node_modules/pyright/package.json', import.meta.url)), 'utf8'));
  console.log(`✓ 可选 Python 语义引擎：Pyright ${metadata.version}（运行时需要 Node.js；无需执行项目代码）`);
} catch {
  console.log('· 可选 Python 语义引擎未安装：npm ci 后可用；静态关系阅读不受影响。');
}
console.log(failed ? '\n补齐缺项后重新运行 npm run doctor。浏览器演示只需要 Node.js。\n' : '\n环境就绪。运行 npm run desktop 启动桌面阅读器。\n');
process.exitCode = failed ? 1 : 0;

