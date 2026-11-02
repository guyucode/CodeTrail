// Isolated browser demo: real React/Monaco, no native transport or user project data.
import { chromium, expect } from '@playwright/test';
const browser = await chromium.launch({ executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH, headless: true });
try {
  const page = await browser.newPage();
  await page.goto(process.env.BASE_URL || 'http://127.0.0.1:1420');
  const input = page.getByRole('textbox', { name: '搜索函数、变量、功能或文件' });
  await input.fill('choose_route');
  const results = page.getByRole('region', { name: '代码搜索结果' });
  await expect(results.getByRole('button').filter({ hasText: 'function · agent/model_router.py:' })).toHaveCount(1);
  await expect(results.getByRole('button').filter({ hasText: 'def choose_route' })).toHaveCount(1);
  await results.getByRole('button').filter({ hasText: 'def choose_route' }).click();
  await expect(page.getByRole('tab', { name: '完整代码', exact: true })).toHaveAttribute('aria-selected', 'true');
  await expect(page.locator('.active-file-tab')).toContainText('model_router.py');
  await input.fill('max_attempts');
  await expect(results.getByRole('button').filter({ hasText: 'max_attempts' }).first()).toBeVisible();
  await results.getByRole('button').filter({ hasText: 'max_attempts' }).first().click();
  await expect(page.locator('.editor-container .monaco-editor .view-lines').first()).toContainText('max_attempts');
  await input.fill('definitely_missing_token_492');
  await expect(results.getByText('当前搜索范围内没有文字匹配。')).toBeVisible();
  await page.getByRole('button', { name: '清除搜索' }).click();
  await expect(results).toHaveCount(0);
  await expect(page.getByRole('button', { name: '查看功能：模型路由', exact: true })).toBeVisible();
  console.log('PASS function definitions, literal variable matches, source navigation, stale results cleared, original feature navigation restored (browser demo only)');
} finally { await browser.close(); }
