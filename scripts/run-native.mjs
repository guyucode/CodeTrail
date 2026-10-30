// Run under an X11 desktop or xvfb-run so both driver and test share DISPLAY.
import { spawn } from 'node:child_process';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';
const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const port = Number(process.env.CODETRAIL_TEST_PORT ?? '4444');
if (!Number.isInteger(port) || port < 1024 || port > 65533) throw new Error('Invalid CODETRAIL_TEST_PORT');
if (!process.env.DISPLAY) throw new Error('Use xvfb-run -a dbus-run-session -- npm run test:native:managed');
const endpoint = `http://127.0.0.1:${port}`;
try {
  const response = await fetch(`${endpoint}/status`, { signal: AbortSignal.timeout(700) });
  if (response.ok) throw new Error(`A driver is already running on ${port}; choose CODETRAIL_TEST_PORT.`);
} catch (error) { if (error.message?.startsWith('A driver')) throw error; }
const driver = spawn(process.env.TAURI_DRIVER ?? 'tauri-driver', ['--port', String(port), '--native-port', String(port + 1)], { cwd: root, stdio: 'inherit' });
let startupError;
driver.on('error', error => { startupError = error; });
try {
  const deadline = Date.now() + 10000;
  let ready = false;
  while (Date.now() < deadline) {
    if (startupError) throw startupError;
    if (driver.exitCode !== null) throw new Error('tauri-driver exited during startup');
    try { ready = (await fetch(`${endpoint}/status`, { signal: AbortSignal.timeout(500) })).ok; } catch {}
    if (ready) break;
    await new Promise(resolve => setTimeout(resolve, 100));
  }
  if (!ready) throw new Error('tauri-driver did not start');
  const test = spawn(process.execPath, ['scripts/smoke-native.mjs'], { cwd: root, stdio: 'inherit', env: { ...process.env, WEBDRIVER_URL: endpoint } });
  process.exitCode = await new Promise((resolve, reject) => { test.on('error', reject); test.on('exit', code => resolve(code ?? 1)); });
} finally {
  driver.kill('SIGTERM');
}
