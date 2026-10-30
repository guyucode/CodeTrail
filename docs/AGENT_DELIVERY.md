# Agent 交付协议 v1

Agent 在写代码时维护功能与证据，CodeTrail 展示、校验并保存人的阅读确认。
本协议不执行源码，不提供模型 API 或自动测试执行能力。

## Agent 的工作流程

1. 修改前运行 `agent inspect`，复用现有功能 ID；不要按名称重新创建 ID。
2. 完成代码修改后再次 inspect，取得最新 `revision`、功能 `currentDigest`、文件哈希和符号 ID。
3. 构造交付 JSON，通过 `agent deliver` 写入。本次涉及的功能必须提交完整证据列表。
4. 运行 `agent check`。交付后源码又变化时重新 inspect、更新交付；不要修改人的审阅字段。
5. 最终回复列出功能 ID、变更说明与实际测试记录。职责为 `test` 的关联只代表阅读证据，不代表测试通过。

构建与调用：

```bash
cargo build -p reader-cli --locked
./target/debug/reader-cli agent inspect /absolute/repository > inventory.json
./target/debug/reader-cli agent deliver /absolute/repository < delivery.json
./target/debug/reader-cli agent check /absolute/repository
```

也可以使用 `cargo run -q -p reader-cli -- agent ...`。
输出为单个 JSON 文档；成功退出码为 0，参数/校验错误为 1，check 发现过期证据为 2。
`inspect` 输出 `revision`、`fileHashes` 和完整 `project`（包含 symbols 和 features）。
deliver 从标准输入读取完整 JSON（调用者需关闭 stdin），最大 2 MiB。
inspect/check 不创建 `.reader/`。

## 请求示例

```json
{
  "schemaVersion": 1,
  "deliveryId": "routing-change-001",
  "expectedRevision": "复制 inventory.revision",
  "features": [
    {
      "id": "feature-model-routing",
      "expectedDigest": null,
      "name": "模型路由",
      "description": "按任务复杂度选择模型，并保留降级路径。",
      "summary": "新增本地与云端路由决策。",
      "evidence": [
        {
          "id": "routing-implementation",
          "path": "agent/model_router.py",
          "symbolId": "agent/model_router.py::choose_route",
          "expectedFileHash": "复制 inventory.fileHashes 对应路径的值",
          "role": "implementation",
          "responsibility": "根据复杂度与偏好选择路由。"
        }
      ]
    }
  ]
}
```

- ID 允许字母、数字、`-`、`_`，最长 128 字节。功能改名仍复用原 ID；证据职责没变时复用证据 ID。
- 新功能 `expectedDigest: null`；更新或显式接管旧人工功能，必须使用该功能当前 `currentDigest`。
- `expectedRevision` 是整个 Agent 存储的更新令牌，防止旧 Agent 覆盖新交付。
- 每批 upsert 1–500 个功能；未提交的功能保持不变。每个功能须有 1–200 条证据。
- v1 不支持删除、退役、设置层级/颜色或自动推断功能；已有功能的层级/颜色保留。
- `symbolId` 从 inspect 的 symbols 获取；默认绑定整个符号。行范围关联需提供正整数 `startLine`、`endLine`，含首尾行；可附属 `symbolId` 限制所属范围。禁止路径越界与符号链接扩张。
- 每条证据必须带 `role` 和非空 `responsibility`。角色为 `entry`（入口）、`implementation`（实现）、`constraint`（约束）、`errorHandling`（异常处理）、`test`（测试）。
- 请求拒绝未知字段，不能提交 `reviewState`、`reviewedDigest`、`reviewedAt`、人工 `note` 等字段。

## 保存与兼容

Agent 批次原子保存在 `.reader/agent.json`，独立 `schemaVersion: 1`。
现有 `.reader/index.json` 和 `.reader/features/<id>.json` 原样兼容，不进行强制迁移。
读取时按功能 ID 合并：Agent 提供功能说明与代码关联，旧功能文件提供人工审阅、颜色、层级，以及按证据 ID 匹配的人工备注。
Agent 更新绝不写入人工功能文件；人确认时仍由 CodeTrail 写入人工文件。
改名不会改变 ID，重复提交相同证据不会新增重复功能。过期请求需重新 inspect 后再提交。
本格式要求新版 CodeTrail；旧版无法理解 agent.json，不用于维护 Agent 交付。

Agent 管理的功能在桌面中不能再手动编辑说明、删除、重绑或解绑；旧人工功能保持原操作。
人工审阅入口保持可用。Agent CLI 是职责分离的协议接口，不是抵御拥有仓库文件写权限者的安全隔离：旧 RPC 的人工审阅接口仍兼容，Agent 应只调用 agent 子命令。

## 摘要和状态

交付时校验每个文件的 SHA-256、现有功能阅读摘要和存储 revision；任一项错误拒绝整批，保留旧交付。
正常跳过二进制资源属于信息提示，不阻断交付或 check；损坏标注、扫描超限等其他索引提示仍会阻断。证据仍必须指向成功索引的文本文件。
提交前再次读取证据文件确认哈希。源码与元数据不是同一个文件系统事务；并发写源码仍可能发生，重新读取或 check 会显示过期结果，不能据此断言源码不会再变。
证据摘要覆盖功能说明、稳定证据身份、角色、职责和当前片段内容。仅位置移动且内容不变可继续匹配。
符号缺失、歧义或内容变化会显示“交付证据已变化”，不静默选择同名定义。

人工阅读摘要与交付证据摘要分别计算。Agent 提交永远不会生成“已确认”；已确认内容变了显示“待复核”，保留旧人工确认字段。人可以确认当前源码，同时 Agent 交付仍显示过期；这是两个独立事实。
职责说明变动同样会使旧人工确认过期。旧人工功能没有角色时仍使用原有摘要算法，不制造批量待复核。

交付使用独占 `.reader/agent.lock` 和单文件原子替换。锁存在时拒绝另一个写入；进程异常后需确认无交付进程，再人工清理残留锁。此锁不声称提供所有旧版人工写入接口的跨进程事务。

## 桌面阅读

打开工程后直接看到 Agent 交付功能。功能详情展示稳定 ID、交付 ID/说明、源码一致性、人工审阅状态；每条代码证据展示职责角色与说明，并可跳转。
CodeTrail 验证定位与摘要，不证明功能说明完整、所有路径被覆盖或测试已通过。
多次打开/刷新不改写交付，也不替 Agent 更新哈希；内容变了必须由 Agent 重新交付。

## Git 基线、更新草稿与统一交付检查

项目自身的 Git 初始基线为 `99dc95f`。本地基线不自动上传远程。每次任务开始选定基线提交；检查尚未提交的修改通常使用 `HEAD`，跨提交交付应使用任务开始时记录的提交 SHA，避免提交后差异变空。

### 生成更新草稿

```bash
./target/debug/reader-cli agent draft /absolute/repository feature-id > /tmp/delivery.json
# 不传功能 ID 时，为全部已有 Agent 功能生成草稿。
```

草稿复用功能和证据 ID、说明和职责，填入最新 revision、currentDigest、文件哈希及当前定位。它不会写入 `.reader/`，也不会自动交付。必须核对说明、补充新增代码、移除失效证据，替换临时 deliveryId 和 TODO 摘要。无法定位的旧证据不能因为填入了最新哈希就视为有效；以 inspect 和人工核对为准。旧人工功能仅在显式指定 ID 时生成草稿，缺失的职责仍需补齐；新增功能按请求格式新增稳定 ID。

### 检查遗漏关联

```bash
./target/debug/reader-cli agent coverage /absolute/repository HEAD > /tmp/coverage.json
```

返回每个变更文件的关联功能、未被有效证据覆盖的非空新增行、删除行数和 `changeDigest`。同文件已有功能不会掩盖其他新增函数的遗漏。覆盖计算包含旧人工功能；Agent 证据是否过期仍由 check 独立判断。删除侧的语义无法仅从当前锚点判断，所以删除、纯重命名和无可表达行差异的改动采取保守策略，要求说明。配置、文档、共用基础设施也可以说明无需关联；不强迫绑定无关功能。Git 对比有警告时拒绝给出完整覆盖结论。

`.reader/` 是交付及人工记录，不计入源码覆盖。此检查不审计人工记录的 Git 改动；Agent 仍须遵守人工隔离规则。覆盖是行范围检查，不证明功能语义完整；未变化的二进制资源不阻断检查；变化的二进制或非 UTF-8 资源目前使差异不完整并阻断检查，不能靠理由绕过。仅模式变化等无文本行差异需填写理由。

### 统一交付校验

```bash
./target/debug/reader-cli agent validate /absolute/repository < /tmp/check-plan.json > /tmp/check-result.json
```

调用者须明确准备以下检查计划，输入最多 2 MiB，未知字段拒绝：

```json
{
  "schemaVersion": 1,
  "base": "任务开始时的提交 SHA 或 HEAD",
  "unassociated": [
    {
      "path": "README.md",
      "changeDigest": "复制 coverage 中对应文件的摘要",
      "reason": "仅更新使用说明，不新增产品行为。"
    }
  ],
  "checks": [
    {"program": "cargo", "args": ["test", "--workspace", "--locked"]},
    {"program": "npm", "args": ["test"]},
    {"program": "npm", "args": ["run", "build"]}
  ]
}
```

`unassociated` 每个需说明文件恰好一条、理由非空；摘要绑定实际差异，旧理由、重复和多余理由拒绝。解决遗漏时优先添加真实职责证据，不能把所有改动一律写成“无需关联”。

validate 先校验交付摘要与遗漏说明，通过后按顺序在目标根目录执行明确提供的程序及参数（直接启动进程，不隐式套 shell）。检查列表不能为空；按修改选择真实测试，不以空操作代替测试。**计划具有执行代码的能力，只运行经核对的计划，不直接运行仓库或第三方提供的未知计划。** CLI 不自动发现或执行项目脚本。调用者可以明确选择 npm 脚本等命令，并为执行结果负责。

结果以单个 JSON 返回每个命令的输出、退出码与最终状态。所有命令均记录结果；命令失败或不存在不会被当成通过。检查后再次比对源码、交付、人工状态和差异，执行期间发生变化则失败。成功退出 0，校验或检查未通过退出 2，输入或运行框架错误退出 1。此过程不写阅读确认、不证明测试充分，也无法保证检查结束后没有其他进程继续修改文件。

推荐顺序：记录 Git 基线 → inspect → 修改与测试 → draft 并核对 → deliver → coverage 并填写必要说明 → validate → 桌面验证。桌面验证后若修改源码，再交付并重新 validate。检查计划与结果放在仓库外；不要把运行日志作为产品源码关联。

## 项目概览与运行路径（v1 可选扩展）

交付根对象可以携带可选 `overview`，与功能证据一起经过 revision 校验后原子保存。旧请求省略或传 null 时保留现有概览；旧 `.reader/agent.json` 没有概览仍可读取。`agent inspect` 在 `project.overview` 返回概览，`agent draft` 保留它。该扩展需要支持概览的新版 CodeTrail；不要用旧版工具改写含概览的存储。

```json
{
  "overview": {
    "summary": "项目解决什么问题，模块如何共同完成任务。",
    "modules": [
      {"id": "core", "name": "核心引擎", "responsibility": "扫描源码并提供可追溯的阅读结果。"}
    ],
    "paths": [
      {
        "id": "open-project",
        "name": "打开项目",
        "description": "从工程目录到可阅读的代码。",
        "steps": [
          {
            "id": "scan", "title": "扫描源码", "description": "读取受限文本并提取符号。",
            "moduleId": "core", "featureId": "已有稳定功能 ID", "evidenceId": "该功能内已有证据 ID"
          }
        ]
      }
    ]
  }
}
```

以上是交付对象的局部示例；仍须填写 schemaVersion、deliveryId、expectedRevision 和至少一个完整功能。概览不能设置审阅字段，不自动确认任何步骤。

- 模块 1–20 个、路径 1–10 条，每条路径 1–20 个步骤；项目自身使用三条核心路径，避免把所有函数堆入概览。
- 模块/路径 ID 在各自集合中唯一；步骤 ID 在路径内唯一，遵守现有 ID 规则。改名时保留身份。
- summary 非空且最多 4000 字符；模块名、路径名及步骤标题最多 100 字符；模块职责最多 1000 字符，路径/步骤说明最多 2000 字符。
- moduleId 必须存在；featureId + evidenceId 必须精确指向本次交付后的有效身份。替换被路径引用的证据时，必须同时更新对应步骤，否则拒绝整批写入。
- 引用合法只证明能定位到声明，不证明运行顺序、输入输出或分支的语义正确。Agent 必须阅读实现，维护每一步的职责、主要数据流与必要分支，不能把解释性路径冒充经过运行追踪验证的调用图。
- `agent check` 和 `agent validate` 会拒绝无效概览引用或损坏结构。源码证据变化继续由原摘要机制校验；概览文字本身不属于人的功能阅读摘要，修改概览不伪造或清除人工确认。
- 桌面在有概览时默认打开概览；点击步骤直接定位其功能和证据。缺失/歧义证据不可跳转，变化证据提示核对；保留“项目概览”返回入口。无概览的旧项目继续直接阅读功能。损坏概览显示索引警告、保留源码阅读能力。

## 当前加载项目的修改动态（可选独立记录）

右侧 Agent 动态只表示**当前加载项目的修改**。加载 local_model_chat 时，只报告该项目源码/配置/文档的改动；CodeTrail 自身开发只向 code-reader 报告。程序运行、中间变量或采集验证属于运行记录；阅读导入、静态讲解和 .reader 元数据更新不充当项目修改。

桌面右侧每 2 秒读取当前打开根目录的记录，不重新扫描源码、不跳转阅读位置。Agent 必须主动报告，文件变化本身不能推断意图或效果。记录时间由 CLI 写入；它表示收到报告的时间，不证明 Agent 仍在运行。界面始终区分“预期效果”与“Agent 报告的验证结果”，报告不等于 CodeTrail 独立验证。

```bash
./target/debug/reader-cli agent report /absolute/repository <<'JSON'
{
  "projectRoot": "/absolute/repository",
  "kind": "projectChange",
  "files": ["src/chat.py"],
  "id": "task-001-working",
  "task": "完善聊天失败提示",
  "stage": "working",
  "places": ["聊天提交入口", "失败提示"],
  "impact": "影响聊天请求失败后的提示，不改变成功路径。",
  "expectedEffect": "让使用者知道失败原因和下一步操作。",
  "verification": null
}
JSON
```

新 report 必须提供 projectRoot、kind、files。projectRoot 须与目标项目规范化后的绝对根目录完全一致，可从 agent inspect 的 project.root 获取；仅共享路径前缀不算同一项目。kind 仅接受 projectChange。files 为 1–30 个具体项目内相对文件路径，禁止绝对路径、越界、符号链接、目录、通配符、重复路径及 .reader/.git 元数据；允许将要新建或已删除的文件。places 仍用于便于理解的业务位置说明。文件列表证明声明的范围，不自动证明实际语义、修改发生或测试充分；Agent 必须如实报告。

读取时，只有归属当前规范根目录且为 projectChange、文件范围合法的记录才展示；前后端均检查。旧版缺少范围字段的记录继续兼容读取并原样保留，但归属未确认，不自动改写、归类或展示。复制记录到别的项目或移动项目目录，不会静默改变其声明归属。没有匹配报告时显示“暂无修改动态”，不拿阅读器自身工作或运行回执填补。

根对象拒绝未知字段。id 为 1–128 字节字母/数字/连字符/下划线，必须唯一；stage 为 working/checking/delivered/blocked；task 最多 200 字符，places 为 1–30 条且每条最多 500 字符；impact、expectedEffect 和可选 verification 最多 4000 字符且非空。recordedAt 可省略，写入时由服务端覆盖为 Unix 秒。每次阶段切换追加新 ID，不覆盖历史。验证结果应写实际命令/操作、结果及限制；未验证时留 null。保留最新 200 条，整个文件最多 2 MiB，超限拒绝且保留旧数据。

记录单独原子保存在 `.reader/activity.json`（事件数组），不修改 agent.json、人工阅读状态或批注。与批注写入共享 journal.lock；短暂占用会等待最多约 1 秒，异常残留锁须确认无写入进程后人工清理。report 只负责进展，不替代最终功能交付与统一校验。历史事件不做源码摘要更新，不声称当前源码仍与旧结果一致。

## 人工批注与 Markdown 反馈

阅读器右侧“记批注”可在项目概览、功能和源码阅读中使用。正文停笔约 600 毫秒后保存；有源码位置时冻结当时的功能、路径、行范围、SHA-256 和选中片段，无源码时只记录阅读上下文。当前源码后续移动或变化不会自动改写历史批注，也不使用旧行号断言当前代码位置。

批注单独保存在 `.reader/annotations.json`（数组），每条有 id/body/context/path/startLine/endLine/fileHash/featureId/featureName/quote/version。最多 1000 条，正文最多 20000 字符，引用最多 4000 字符，文件最多 2 MiB。保存和删除必须提供该条 expectedVersion，新增使用空字符串，成功保存生成新版本；冲突保留原记录与本机草稿，不自动覆盖。空正文不写入。损坏文件明确报错，不重建覆盖。所有写入拒绝越界和符号链接。

输入先保留当前应用的本机草稿，保存成功清除；保存失败、切换项目或退出后可在同一应用、同一路径重新打开恢复。恢复草稿版本与项目记录冲突时另存为新批注，请人工核对。草稿不随项目搬移、不是跨设备备份。未保存内容会阻止 Markdown 导出。人工修改冲突不会自行合并。

“导出 Markdown”原子覆盖当前项目 `.reader/annotations.md`，包含全部已保存批注、上下文、历史位置、版本及引用。界面显示完整文件路径，使用者手动发送文件给 Agent；不自动发送、不自动完成、不修改原批注或阅读确认。Agent CLI 不提供批注写入入口。拥有仓库写权限仍能直接访问文件，这属于工作规则隔离。

旧项目无需迁移，缺少记录文件视为空。新增记录不影响既有 `.reader/features/`、`.reader/agent.json` 及已有功能 ID。

## 实际运行记录

运行记录独立使用 `trace start/append/finish/list/read`，格式、Python 接入和历史版本规则见 [RUNTIME_RECORDS.md](RUNTIME_RECORDS.md)。它不属于功能交付、Agent 进度或人工审阅。功能和概览负责解释与导航；运行记录保存采集器实际观察到的输入、输出和分支，不能把 Agent 文字推演转为“实际执行”。
