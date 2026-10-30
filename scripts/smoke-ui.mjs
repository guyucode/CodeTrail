#!/usr/bin/env node

/**
 * Repeatable browser smoke test for the bundled demo.
 *
 * Start `npm run dev` (or `npm run preview`) in another terminal, then run:
 *   npm run test:ui
 *
 * BASE_URL optionally overrides http://127.0.0.1:1420.
 * PLAYWRIGHT_BROWSERS_PATH is managed externally by Playwright/the caller.
 * This test creates a fresh browser context and never opens a real repository.
 * No screenshots, traces, or source-code writes are produced.
 */

import { chromium, expect as baseExpect } from "@playwright/test";

const baseUrl = process.env.BASE_URL || "http://127.0.0.1:1420";
const expect = baseExpect.configure({ timeout: 15_000 });
const featureName = "UI 验收 · 失败降级";
const featureDescription = "验证功能创建、符号绑定、审阅和重新打开后的恢复。";
const completed = [];
const pageErrors = [];
let browser;
let context;

async function check(name, action) {
  await action();
  completed.push(name);
  console.log(`PASS ${name}`);
}

try {
  browser = await chromium.launch({ channel: process.env.PLAYWRIGHT_CHANNEL, headless: true, args: ["--no-sandbox"] });
  context = await browser.newContext({
    viewport: { width: 1440, height: 1000 },
  });
  const page = await context.newPage();
  page.setDefaultTimeout(15_000);
  page.setDefaultNavigationTimeout(45_000);
  page.on("pageerror", (error) => pageErrors.push(error.message));

  async function more(name) {
    await page.locator(".reading-tools > summary").click();
    await page.locator(".reading-tools-menu").getByRole("button", { name, exact: true }).click();
  }
  async function showDetails() {
    if (await page.locator(".inspector").count() === 0) await more("查看详情");
  }
  async function diffSettings() {
    if (!(await page.locator(".comparison-options").evaluate(e => e.open))) {
      await page.locator(".comparison-options > summary").click();
    }
  }
  await check("内置演示加载，并使用真实 Monaco 编辑器", async () => {
    await page.goto(baseUrl, { waitUntil: "domcontentloaded" });
    await expect(
      page.getByRole("button", { name: "打开仓库", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".demo-tag")).toHaveText("演示项目");
    await expect(page.locator(".inspector")).toHaveCount(0);
    await expect(page.getByRole("tab", { name: "功能地图", exact: true })).toHaveCount(0);
    await expect(page.getByRole("button", { name: "确认已读", exact: true })).toBeVisible();
    await expect(page.getByRole("tab", { name: "本次改动", exact: true })).toHaveAttribute("aria-selected", "true");
    await page.getByRole("tab", { name: "完整代码", exact: true }).click();
    await showDetails();
    await expect(
      page.locator(".editor-container .monaco-editor .view-lines").first(),
    ).toBeVisible();
    await expect(page.locator(".active-file-tab")).toContainText(
      "model_router.py",
    );
  });

  await check("创建功能，程序化跳转不覆盖新功能的选择", async () => {
    await page
      .getByRole("button", { name: "查看功能：流式输出与降级", exact: true })
      .click();
    await expect(page.locator(".feature-overview h2")).toHaveText(
      "流式输出与降级",
    );
    await more("新建功能");
    await page.getByLabel("功能名称", { exact: true }).fill(featureName);
    await page.getByLabel("功能说明", { exact: true }).fill(featureDescription);
    await page.getByRole("button", { name: "创建功能", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.locator(".feature-overview h2")).toHaveText(featureName);
    await expect(
      page.locator(".editor-container .monaco-editor .view-lines").first(),
    ).toBeVisible();
    await expect(page.locator(".anchor-card")).toHaveCount(0);
  });

  await check("将当前函数绑定到新功能", async () => {
    await page.getByRole("tab", { name: "完整代码", exact: true }).click();
    await more("绑定当前片段");
    const dialog = page.getByRole("dialog", {
      name: "绑定代码片段",
      exact: true,
    });
    await expect(dialog).toContainText("stream_with_fallback");
    await expect
      .poll(() =>
        dialog
          .locator("select")
          .evaluate((select) => select.selectedOptions[0]?.textContent?.trim()),
      )
      .toBe(featureName);
    await page
      .getByLabel("阅读备注（可选）", { exact: true })
      .fill("检查失败降级时不会重复已输出内容。");
    await page.getByRole("button", { name: "确认绑定", exact: true }).click();
    await expect(page.getByRole("dialog")).toHaveCount(0);
    await expect(page.locator(".anchor-card")).toHaveCount(1);
    await expect(page.locator(".anchor-card")).toContainText(
      "stream_with_fallback",
    );
    await expect(page.locator(".feature-overview h2")).toHaveText(featureName);
  });

  await check("确认当前版本", async () => {
    await page
      .getByRole("button", { name: "确认已读", exact: true })
      .click();
    await expect(page.locator(".feature-overview .status-badge")).toHaveText(
      "已读",
    );
  });

  await check("重新加载后，功能绑定与审阅状态持久化", async () => {
    await page.reload({ waitUntil: "domcontentloaded" });
    await showDetails();
    await page
      .getByRole("button", { name: `查看功能：${featureName}`, exact: true })
      .click();
    await expect(page.locator(".feature-overview h2")).toHaveText(featureName);
    await expect(page.locator(".feature-overview .status-badge")).toHaveText(
      "已读",
    );
    await expect(page.locator(".anchor-card")).toHaveCount(1);
    await expect(page.locator(".anchor-card")).toContainText(
      "检查失败降级时不会重复已输出内容。",
    );
  });

  await check("Monaco worker 实际计算并显示新增与删除行", async () => {
    await page.getByRole("tab", { name: "本次改动", exact: true }).click();
    await expect(page.locator(".monaco-diff-editor")).toBeVisible();
    // A mounted diff editor alone does not prove its worker has computed a diff.
    await expect(
      page.locator(".monaco-diff-editor .line-insert").first(),
    ).toBeVisible({ timeout: 30_000 });
    await expect
      .poll(() => page.locator(".monaco-diff-editor .line-delete").count(), {
        timeout: 30_000,
      })
      .toBeGreaterThan(0);
    await expect(page.locator(".monaco-diff-editor")).toHaveClass(
      /monaco-diff-editor/,
    );
    await expect(page.locator(".editor-footer")).toContainText(
      "HEAD → 当前工作区",
    );
  });

  await check("切换统一差异视图", async () => {
    await diffSettings();
    await page
      .getByRole("button", { name: "切换并排对比", exact: true })
      .click();
    await expect(page.locator(".editor-footer")).toContainText("并排对比");
    await expect(
      page.getByRole("button", { name: "切换统一对比", exact: true }),
    ).toBeVisible();
    await expect(page.locator(".monaco-diff-editor")).toHaveClass(
      /side-by-side/,
    );
  });

  await check("生成包含新功能、代码证据和状态的阅读报告", async () => {
    await page
      .getByRole("button", { name: "导出阅读记录", exact: true })
      .click();
    const dialog = page.getByRole("dialog", {
      name: "阅读与审阅记录",
      exact: true,
    });
    await expect(dialog.locator(".report-preview")).toContainText(featureName);
    await expect(dialog.locator(".report-preview")).toContainText(
      "stream_with_fallback",
    );
    await expect(dialog.locator(".report-preview")).toContainText("已确认");
    await dialog
      .getByRole("button", { name: "关闭对话框", exact: true })
      .click();
  });

  await check("直接点击代码，反向定位到所属功能", async () => {
    await page
      .getByRole("button", { name: "查看功能：工具调用", exact: true })
      .click();
    await expect(page.locator(".feature-overview h2")).toHaveText("工具调用");
    await page.locator(".secondary-navigation > summary").click();
    await page.getByRole("tab", { name: "代码", exact: true }).click();
    await page
      .locator(".file-nav-item")
      .filter({ hasText: "model_router.py" })
      .click();
    // Monaco virtualizes lines. Click a known visible line at a specific local
    // offset rather than the first rendered line, which may be outside its clip.
    await page
      .locator(".monaco-editor .view-line")
      .filter({ hasText: "def choose_route" })
      .click({ position: { x: 90, y: 10 } });
    await expect(page.locator(".feature-overview h2")).toHaveText("模型路由");
    await expect(page.locator(".cursor-features")).toContainText("模型路由");
  });

  await check("实际键入文本无法修改只读代码", async () => {
    const codeLines = page
      .locator(".editor-container .monaco-editor .view-lines")
      .first();
    // Check the focused source line rather than Monaco's entire virtualized
    // viewport, whose rendered line set can change while smooth scrolling ends.
    const sourceLine = page
      .locator(".editor-container .monaco-editor .view-line")
      .filter({ hasText: "def choose_route" });
    const before = await sourceLine.innerText();
    await page.keyboard.type("THIS_SHOULD_NOT_CHANGE");
    await expect(codeLines).not.toContainText("THIS_SHOULD_NOT_CHANGE");
    await expect.poll(() => sourceLine.innerText()).toBe(before);
    await expect(page.locator(".read-only-label")).toHaveText("只读");
  });

  await check("反复切换差异与源码，模型释放后仍能正常阅读", async () => {
    for (let attempt = 0; attempt < 3; attempt += 1) {
      await page.getByRole("tab", { name: "本次改动", exact: true }).click();
      await expect(
        page.locator(".monaco-diff-editor .line-insert").first(),
      ).toBeVisible();
      await page.getByRole("tab", { name: "完整代码", exact: true }).click();
      await expect(page.locator(".active-file-tab")).toContainText(
        "model_router.py",
      );
      await expect(
        page.locator(".editor-container .monaco-editor .view-lines").first(),
      ).toContainText("def choose_route");
    }
  });

  async function selectRelationSymbol(name) {
    const picker = page.getByLabel("分析关系的符号", { exact: true });
    const value = await picker
      .locator("option")
      .filter({ hasText: `${name} ·` })
      .first()
      .getAttribute("value");
    if (!value) throw new Error(`Missing relation symbol: ${name}`);
    await picker.selectOption(value);
  }

  await check("相关逻辑展示实际调用、引用和外部边界", async () => {
    await more("相关逻辑");
    await selectRelationSymbol("choose_route");
    await expect(page.locator(".relation-graph")).toBeVisible();
    await expect(page.locator(".incoming-relations")).toContainText(
      "stream_chat",
    );
    await expect(page.locator(".outgoing-relations")).toContainText("Route");
    await expect(page.locator(".reference-relations")).toContainText(
      "get_route_callback",
    );
    await expect(page.locator(".incoming-relations")).not.toContainText(
      "get_route_callback",
    );
    await expect(page.locator(".boundary-relations")).toContainText(
      "ValueError",
    );
    await expect(page.locator(".graph-current")).toHaveClass(/node-changed/);
    await expect(
      page.getByRole("button", {
        name: "图节点：Route，查看定义",
        exact: true,
      }),
    ).not.toHaveClass(/node-changed/);
    await expect
      .poll(() => page.locator(".relation-connectors > path").count())
      .toBeGreaterThan(1);
    await expect(page.locator(".relation-snapshot")).toContainText(
      "当前工作区",
    );
  });

  await check(
    "直接与可能影响分别显示，未改动被调用方不传播且审阅状态保留",
    async () => {
      const possible = page
        .locator(".relation-impact.impact-possible")
        .filter({ hasText: "对话入口" });
      await expect(possible).toContainText("可能受影响");
      await expect(possible.locator(".impact-review")).toHaveText("已确认");
      await expect(
        page
          .locator(".relation-impact.impact-direct")
          .filter({ hasText: "模型路由" }),
      ).toBeVisible();
      await expect(page.locator(".relation-impacts")).not.toContainText(
        "路由数据结构",
      );
      await possible
        .getByRole("button", { name: "对话入口", exact: true })
        .click();
      await expect(page.locator(".feature-overview h2")).toHaveText("对话入口");
      await expect(page.locator(".feature-overview .status-badge")).toHaveText(
        "已确认",
      );
      await expect(page.locator(".relation-graph")).toBeVisible();
    },
  );

  await check("点击调用方图节点，核验证据并保留功能选择", async () => {
    await page
      .getByRole("button", {
        name: "图节点：stream_chat，查看调用证据",
        exact: true,
      })
      .click();
    await expect(page.locator(".active-file-tab")).toContainText(
      "chat_entry.py",
    );
    await expect(
      page.locator(".editor-container .monaco-editor .view-lines").first(),
    ).toContainText("select_route(complexity)");
    await expect(page.locator(".editor-footer")).toContainText("已选 L");
    await expect(page.locator(".relation-source-banner")).toContainText(
      "choose_route",
    );
    await expect(page.locator(".feature-overview h2")).toHaveText("对话入口");
    await page
      .getByRole("button", { name: "返回相关逻辑", exact: true })
      .click();
    await expect(page.locator(".graph-current")).toContainText("choose_route");
  });

  await check("点击被调用方图节点打开真实定义，随后返回原关系", async () => {
    await page
      .getByRole("button", { name: "图节点：Route，查看定义", exact: true })
      .click();
    await expect(page.locator(".active-file-tab")).toContainText(
      "model_router.py",
    );
    await expect(
      page.locator(".editor-container .monaco-editor .view-lines").first(),
    ).toContainText("class Route");
    await expect(page.locator(".editor-footer")).toContainText("已选 L");
    await expect(page.locator(".feature-overview h2")).toHaveText("对话入口");
    await page
      .getByRole("button", { name: "返回相关逻辑", exact: true })
      .click();
    await expect(page.locator(".graph-current")).toContainText("choose_route");
  });

  await check("非调用引用可独立打开对应源码证据", async () => {
    await page
      .locator(".reference-relations")
      .getByRole("button", {
        name: "查看引用位置：get_route_callback → choose_route",
        exact: true,
      })
      .click();
    await expect(page.locator(".active-file-tab")).toContainText(
      "chat_entry.py",
    );
    await expect(
      page.locator(".editor-container .monaco-editor .view-lines").first(),
    ).toContainText("return select_route");
    await expect(page.locator(".editor-footer")).toContainText("已选 L");
    await page
      .getByRole("button", { name: "返回相关逻辑", exact: true })
      .click();
    await expect(page.locator(".reference-relations")).toContainText(
      "get_route_callback",
    );
  });

  await check("可选语义增强明确报告可用范围，不伪装语言服务", async () => {
    await page
      .getByRole("button", { name: "Python 语义增强", exact: true })
      .click();
    await expect(page.locator(".relation-warnings")).toContainText(
      "浏览器演示不启动语言服务器",
    );
    await expect(page.locator(".relation-graph")).toBeVisible();
  });

  await check("动态目标保留未解析边界，不生成虚构调用节点", async () => {
    await selectRelationSymbol("dispatch_plugin");
    await expect(page.locator(".graph-current")).toContainText(
      "dispatch_plugin",
    );
    await expect(page.locator(".boundary-relations")).toContainText("handler");
    await expect(page.locator(".boundary-relations")).toContainText("未解析");
    await expect(page.locator(".outgoing-relations .relation-row")).toHaveCount(
      0,
    );
    await expect(
      page.locator(".outgoing-nodes .relation-graph-node"),
    ).toHaveCount(0);
  });

  await check("不支持的语言明确提示，不显示空图冒充分析结果", async () => {
    await selectRelationSymbol("appendToken");
    await expect(page.locator(".relation-unsupported")).toContainText(
      "此语言暂未提供关系分析",
    );
    await expect(page.locator(".relation-graph")).toHaveCount(0);
    await selectRelationSymbol("choose_route");
    await expect(page.locator(".relation-graph")).toBeVisible();
  });

  await check("历史差异不冒充历史调用关系，可明确切回当前工作区", async () => {
    await page.getByRole("tab", { name: "本次改动", exact: true }).click();
    await diffSettings();
    await page.getByLabel("对比目标", { exact: true }).fill("HEAD");
    await page.getByRole("button", { name: "比较", exact: true }).click();
    await more("相关逻辑");
    await expect(page.locator(".relation-historical")).toContainText(
      "不能据此推断历史调用或历史影响",
    );
    await expect(page.locator(".relation-graph")).toHaveCount(0);
    await expect(page.locator(".relation-impacts")).toHaveCount(0);
    await page
      .getByRole("button", { name: "分析当前工作区", exact: true })
      .click();
    await expect(page.locator(".relation-graph")).toBeVisible();
    await expect(page.locator(".relation-snapshot")).toContainText(
      "HEAD → WORKTREE",
    );
  });

  await check("所有交互均未产生 pageerror", async () => {
    expect(pageErrors).toEqual([]);
  });

  console.log(
    JSON.stringify(
      {
        ok: true,
        baseUrl,
        passed: completed.length,
        checks: completed,
        pageErrors,
      },
      null,
      2,
    ),
  );
} catch (error) {
  console.error(
    JSON.stringify(
      {
        ok: false,
        baseUrl,
        passed: completed.length,
        checks: completed,
        pageErrors,
      },
      null,
      2,
    ),
  );
  console.error(error instanceof Error ? error.stack : error);
  process.exitCode = 1;
} finally {
  await context?.close();
  await browser?.close();
}
