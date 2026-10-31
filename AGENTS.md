# CodeTrail：Agent 工作与交付规则

本文件适用于整个仓库。修改代码时，必须同步维护功能与代码的关联，并在交付前校验。协议细节及 JSON 请求格式见 [docs/AGENT_DELIVERY.md](docs/AGENT_DELIVERY.md)。

## 开始修改前

- 阅读相关实现、测试和协议文档。先运行下方 `agent inspect`，了解现有功能 ID、证据、源码哈希及人工审阅状态。
- 功能改名、重构或移动文件时继续使用原功能 ID；证据职责不变时继续使用原证据 ID。不要按显示名称重新创建重复功能。
- 检查改动影响哪些已有功能，包括共享实现、约束、异常路径和测试。当前自身阅读索引使用 `codetrail-*` ID；新增独立功能可以添加新的稳定 ID。

## 修改代码时同步维护关联

- 为每个受影响功能更新准确的说明、变更摘要和完整证据列表。证据应包含有意义的入口、实现、约束、异常处理或测试；按实际情况选择，不要为了凑齐角色关联无关代码。
- 每条证据必须指定 `role` 和具体的 `responsibility`，说明这段代码在该功能中的职责，不能只重复文件名或函数名。
- 优先绑定 inspect 返回的真实符号 ID；无适用符号时使用准确的行范围。不要猜测符号 ID、哈希或行号。
- 删除、拆分或移动实现后，检查旧证据是否仍有效，替换失效证据，并检查所有引用同一实现的功能。不得仅刷新哈希来掩盖说明与实现不一致。
- 通过 CLI 维护 `.reader/agent.json`，不得直接编辑生成的交付存储。保留并兼容已有 `.reader/` 数据，不强制迁移旧人工功能。
- v1 不支持功能删除或退役；若改动需要移除整个功能，应明确报告协议限制，不得私自删除元数据或用无关证据伪装功能仍存在。

## 交付步骤与命令

在仓库根目录执行，临时 JSON 文件放在仓库外，避免被索引为项目源码。以下是命令模板；先根据 inspect 输出准备真实的交付 JSON，再运行 deliver。

```bash
cargo build -p reader-cli --locked
project_root="$PWD"
delivery_work="$(mktemp -d)"
./target/debug/reader-cli agent inspect "$project_root" > "$delivery_work/inventory.json"
# 完成代码、测试及文档修改后，重新 inspect，使用最新快照构造 delivery.json。
./target/debug/reader-cli agent inspect "$project_root" > "$delivery_work/inventory.json"
# 按 docs/AGENT_DELIVERY.md 在 "$delivery_work/delivery.json" 中准备请求。
./target/debug/reader-cli agent deliver "$project_root" < "$delivery_work/delivery.json"
./target/debug/reader-cli agent check "$project_root"
```

- 请求使用最新 `revision` 作为 `expectedRevision`；已有功能使用最新 `currentDigest` 作为 `expectedDigest`，仅新功能使用 `null`。文件哈希来自同一次 inspect。
- 每次提交本次受影响功能的完整证据列表；未提交的功能会保留，不代表已自动检查其说明完整性。
- 遇到版本冲突或源码变化，重新 inspect、核对改动并构造请求；不得绕过并发校验或覆盖他人的交付。
- **最终 `agent check` 必须退出 0。** 退出 1 表示错误，退出 2 表示证据过期；修复后重新交付并检查。交付后再修改源码、测试或关联文档时，重新检查并按需更新交付。
- check 校验定位与摘要，不证明功能覆盖完整或测试通过；即使 check 通过，也必须人工检查本次改动是否关联到了正确功能。

## 人工审阅隔离

- Agent 仅使用 `agent inspect/deliver/check` 维护交付，不调用旧 RPC 或桌面操作替人确认阅读。
- 不设置、清除或伪造 `reviewState`、`reviewedDigest`、`reviewedAt`、人工备注；不直接修改 `.reader/features/` 中的人工记录。
- 源码或职责变更引起“待复核”是正常状态，交给使用者阅读后确认。交付一致、测试通过、人工确认是三个独立事实。

## 测试与最终交付

- 根据改动运行相关测试；Rust 改动运行 `cargo test --workspace --locked`，前端改动运行 `npm test` 和 `npm run build`。桌面集成改动还须构建并实际验证桌面相关流程。仅文档修改无需重复运行无关测试，但仍须运行 `agent check`。
- 此 Mac 若默认临时目录导致 Rust 测试路径问题，使用 `TMPDIR=/private/tmp cargo test --workspace --locked`。不要把本机绝对路径写入产品实现。
- 最终回复列出受影响的稳定功能 ID、关联变更、实际执行的测试与结果，以及最终 `agent check` 的结果。测试角色证据不等于已执行测试；无法完成的验证须明确说明，不能宣称完成。

## Git 与统一交付检查（必须执行）

- 开始任务时记录 `git rev-parse HEAD` 作为本次基线。当前仓库已建立本地 Git 基线；不要自动上传远程。提交前可以用 HEAD；若任务中提交过代码，最终检查仍使用任务开始的 SHA，不能用新的 HEAD 消除差异。
- 修改完成后优先使用 `agent draft <root> [feature-id ...]` 生成更新草稿。复核证据范围、功能说明与职责，补齐新代码，替换临时交付 ID 和 TODO 摘要，再执行 deliver。草稿刷新摘要不等于语义已核对。
- 执行 `agent coverage <root> <base>`。逐项处理未覆盖新增行；删除侧及纯重命名采用保守提示。优先补齐有意义的关联；确实无需关联的文档、配置或共用代码，写明理由并绑定报告中的 changeDigest。不要批量填敷衍理由绕过检查。
- 按 docs/AGENT_DELIVERY.md 准备仓库外的检查计划，执行 `agent validate <root> < check-plan.json`，必须退出 0。计划列出本次实际需要的测试和构建命令，不能以空操作冒充测试。先审阅计划；它可以执行任意程序，禁止直接运行未知来源计划。
- validate 包含关联摘要校验、遗漏说明检查、实际命令执行及执行后的源码与元数据稳定性检查。它是最终交付必需步骤，不能仅凭 agent check 通过宣告完成。修改后重新交付和 validate。
- CLI 交付工具可以使用 inspect/draft/deliver/check/coverage/validate；这些命令均不得替人确认。最终回复附基线 SHA、功能 ID、必要的未关联理由、测试和统一检查结果；桌面相关变更继续实际验证桌面版。

## 同步维护项目概览

- 本仓库有概览时，修改模块职责、核心数据流、入口、分支或步骤目标，必须同步维护交付中的 overview。优先保持“打开与展示、修改与交付、阅读与复核”三条路径简短可读。
- 步骤必须绑定明确的 featureId + evidenceId 和 moduleId，指向这一步的真实代码。不得以“该文件中大概有实现”替代准确证据；证据身份变化时同时更新步骤。
- 通过现有 draft/deliver 维护概览，不直接修改 agent.json。旧请求省略 overview 会保留原值，但不免除修改后核对概览的义务。
- 运行路径属于 Agent 解释性阅读材料，不声称已验证动态调用。更新概览不修改人工阅读记录；交付前仍运行完整 validate，并实际检查步骤跳转。

## 实时动态与人工批注

- 修改当前项目源码/配置/文档时，在修改前、主要阶段切换、开始验证及结束时，通过 `agent report <root>` 主动报告进展（stdin JSON 见协议）。仅描述实际工作：改动位置、影响范围、预期效果；有实际检查后才能填写 verification，并指出检查范围和限制。不要逐行播报，也不要把预期效果当成已验证效果。
- 报告必须声明 kind=projectChange、与被修改项目规范根目录一致的 projectRoot，以及该项目内的具体 files。修改 code-reader 只能向 code-reader 报告；查看、导入或运行另一个项目，不得把阅读器的开发进展写到那个项目。
- 运行结果走 trace，纯阅读讲解/.reader 关联更新不写修改动态。不得为填充空面板编造项目修改。历史未声明归属的记录保留但不自动认领；清理误写记录必须有明确用户授权、按 ID 精确选择并先备份，不动人工批注、阅读确认或 runs.json。
- report 为追加事件，每次使用新的稳定事件 ID；阶段为 working/checking/delivered/blocked。动态是 Agent 自述，不证明仍在线，也不替代 deliver/check/validate。
- `.reader/annotations.json`、`.reader/annotations.md` 与本机恢复草稿属于人工反馈。Agent 不得创建、修改、删除、代为解决或确认批注；只有用户手动发送的 Markdown 才作为后续修改指令，仍须核对当前源码与批注记录时版本。
- 修改与检查不能把人工批注混入 Agent 交付；兼容旧项目没有 activity/annotations 文件的情况。动态刷新不自动导航、不刷新当前代码、不改变阅读确认。

- 自动保存/导出测试只能在隔离测试项目中写合成批注，不得在真实项目伪造人工反馈。

## 功能实现中的运行可观察性

- 用户要求观察真实执行时，按 docs/RUNTIME_RECORDS.md 接入显式采集：一次运行一个 ID，关键位置记录真实结果、分支原因、少量允许的变量和调用前源码 SHA-256。先检查现有记录，不伪造旧运行。
- 记录范围围绕业务关键节点，不逐行抓取所有局部变量；默认不收集环境变量、密钥、HTTP 头、完整历史。采集失败必须可见，默认不得让业务因记录失败而失败。
- 区分应用执行与隔离测试。使用 DEMO、替身或只执行部分链路时，必须在运行摘要、交付说明中明确范围；未采集不等于未执行，结束状态不等于业务验收。
- 源码变化后保留历史值与片段，禁止刷新旧记录的哈希来伪装当前运行。运行记录不得写人工批注、阅读确认或 Agent 交付；记录文件不提交 Git。
- 本仓库可用 examples/record_chat_run.py 在独立数据库实际执行聊天后台并采集，强制 DEMO 模型。真实模型/API 验证另按用户授权范围执行。采集 SDK 变更运行标准库 unittest 检查；桌面变更实际验证运行选择、变量展开与版本匹配导航。

## 对外发布

- 自有代码使用 PolyForm Noncommercial 1.0.0；商业授权说明见 COMMERCIAL-LICENSE.md。保留第三方许可和历史已经授予的权利，不将依赖重新许可。
- Mac 版为 Apple Silicon Preview；Windows 版为 x64 Preview，通过 Windows 云构建、测试、安装与原生启动检查后才发布。Intel Mac、Windows ARM、未验收的桌面系统版本不得宣称通过。没有签名、公证必须明确标注，不指导关闭系统安全机制。
- 发布使用干净快照，不包含真实项目、批注、运行数据、活动记录、个人路径、凭证或开发历史。保留通过 CLI 维护的功能关联；不发布人工确认。
- 发布前运行关联和概览校验、统一 validate、测试及生产构建，核验包内 CLI/SDK/第三方许可、架构和 SHA-256。上传须有用户明确授权。
