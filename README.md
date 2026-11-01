# CodeTrail · 代码阅读器

**按功能读懂 Agent 写的代码，看清改了哪里，记下自己的理解。**

独立的本地桌面应用：从项目概览进入功能和对应代码，阅读改动、记录批注，也能回看显式采集的运行结果。源码区只读；阅读时不需要模型、GPU 或 API Key。

**Windows 0.3 预览版：** 新增无需独立服务器的局域网共读、共享批注、任务整合与耦合分工，见 [局域网协作说明](docs/LAN_COLLABORATION.md)。Windows 0.3 包含上述能力及加载优化；Mac 在线下载仍为 0.2，暂不包含这些能力。

[下载](#下载) · [安装](#安装) · [三步开始使用](#三步开始使用) · [完整使用指南](docs/USER_GUIDE.md)

## 下载

当前下载：**Mac v0.2.0 Preview · Windows v0.3.0 Preview**。请选择你的电脑类型，点击安装包即可；**不要下载 GitHub 的「Code → Download ZIP」或「Source code」来安装软件**，它们是源码。

| 你的电脑 | 直接下载安装包 | 版本与验证说明 |
| --- | --- | --- |
| **Mac · Apple Silicon**（M1 / M2 / M3 / M4 等 M 系列） | **[下载 Mac 版（DMG，约 12 MB）](https://github.com/guyucode/CodeTrail/releases/download/v0.2.0-mac-preview.1/CodeTrail_0.2.0_aarch64.dmg)** | [Mac 发行页](https://github.com/guyucode/CodeTrail/releases/tag/v0.2.0-mac-preview.1) |
| **Windows · x64**（Intel / AMD 64 位） | **[下载 Windows 版（EXE）](https://github.com/guyucode/CodeTrail/releases/download/v0.3.0-windows-preview.1/CodeTrail_0.3.0_windows_x64_setup.exe)** | [Windows 发行页](https://github.com/guyucode/CodeTrail/releases/tag/v0.3.0-windows-preview.1) |

Intel Mac、Windows ARM 和 32 位系统暂不提供安装包。[全部版本与附件](https://github.com/guyucode/CodeTrail/releases)

不确定机型：Mac 点左上角 Apple 菜单 →「关于本机」，查看芯片；Windows 在「设置 → 系统 → 系统信息」查看系统类型。

## 安装

### Mac

1. 下载并打开 `.dmg`，把 **CodeTrail** 拖入「应用程序」。
2. 从「应用程序」打开 CodeTrail，首次会显示演示项目。
3. 本版**未签名、未公证**。若被系统拦截，先核对来源和发行页的 SHA-256，再按 macOS 的「隐私与安全性」提示处理；不要关闭系统安全保护。受管理设备请按组织要求处理。

安装包面向 Apple Silicon，构建部署目标为 macOS 11；目前仅在开发者的 macOS 27 设备验证，其他系统版本尚未逐一验收。

### Windows

1. 下载并双击 `CodeTrail_0.3.0_windows_x64_setup.exe`，按向导为当前用户安装。
2. 若提示缺少 **Microsoft WebView2**，允许安装器联网安装该运行时。
3. 安装后从开始菜单打开 **CodeTrail**，首次会显示演示项目。

本版**没有代码签名**，系统可能提示未知发布者。请核对来源和发行页的 SHA-256，不要关闭系统安全保护。已通过 Windows Server 2022 x64 云端构建、安装和原生窗口检查；Windows 10/11 实机人工验收仍待完成。[详细验证范围](docs/WINDOWS_RELEASE.md)

**基础阅读无需安装 Rust、npm、Python 或模型服务。** Git 改动比较需要 Git；可选 Python 语义增强需要 Node.js 22.12+。普通使用只需下载安装包，CLI / SDK 工具包按需下载。

## 三步开始使用

1. **打开项目**：先体验内置演示，再点右上角「打开仓库」，选择自己的项目文件夹。要看 Git 改动，请选择仓库根目录。
2. **选功能、读代码**：有「项目概览」时先看模块与运行路径，点击步骤进入对应功能和代码；也可以从左侧选功能，用「阅读片段」切换相关实现和测试。
3. **记批注、确认已读**：有疑问就在右侧点「＋ 记批注」，停笔后自动保存；在「我的批注」中导出 Markdown，手动发给 Agent。读完再点「确认已读」，关联内容变化后会提示重读。

### 打开自己的项目，没有功能列表怎么办？

CodeTrail **不会自动猜测业务功能**。没有功能关联的项目仍可浏览文件；要按功能阅读，推荐让编码 Agent 按 [Agent 交付协议](docs/AGENT_DELIVERY.md) 建立并维护关联。也可以通过「更多」创建人工功能并绑定代码片段。

可以把这段话发给正在开发项目的 Agent，并附上协议链接：

> 请按 CodeTrail 的 Agent 交付协议，为当前项目建立简短的功能索引、带职责的代码关联和项目概览。使用 reader-cli 维护并运行校验；以后修改代码时同步更新关联，不替我确认已读。

Agent 使用的同版本工具包（含 `reader-cli`、Python SDK 和协议文档）：[Mac arm64 工具包](https://github.com/guyucode/CodeTrail/releases/download/v0.2.0-mac-preview.1/CodeTrail-tools-0.2.0-macos-arm64.tar.gz) · [Windows x64 工具包](https://github.com/guyucode/CodeTrail/releases/download/v0.3.0-windows-preview.1/CodeTrail-tools-0.3.0-windows-x64.zip)。解压后可使用其中的 CLI；协议中的源码构建步骤仅供开发者。

### Agent 改代码时，怎样看进展？

右侧「Agent 动态」展示**当前加载项目**中 Agent 主动报告的修改位置、影响与预期效果；它不自动监视其他项目，也不自动推断工作内容。代码和索引的外部改动需要点击右上角「刷新代码与索引」。

「运行记录」只显示项目显式接入采集器后留下的结果；阅读器不会自行运行项目或编造中间值。[接入运行记录](docs/RUNTIME_RECORDS.md)

![CodeTrail Windows 桌面版：内置演示项目中的功能、代码和右侧工作面板](https://github.com/guyucode/CodeTrail/releases/download/v0.3.0-windows-preview.1/codetrail-windows-native.png)

*Windows 预览版实际窗口，展示内置演示项目。*

## 数据与使用边界

- 项目功能关联、人工阅读记录、批注和运行记录保存在项目的 `.reader/` 中，建议自行备份；内置演示数据与真实项目隔离。
- 应用不上传源码、不修改源码，也不执行被阅读项目的代码。批注不会自动发送给 Agent。
- 「确认已读」记录的是你阅读过的内容版本，不代表功能正确或测试通过。
- Python 支持局部代码关系；Rust、TypeScript / JavaScript 支持符号阅读。大型仓库和动态关系有分析范围限制，详见使用指南。

## 更多说明

| 你想做什么 | 看这里 |
| --- | --- |
| 详细操作、状态含义与常见问题 | [用户指南](docs/USER_GUIDE.md) |
| 让 Agent 维护功能与代码关联 | [Agent 交付协议](docs/AGENT_DELIVERY.md) |
| 采集真实运行中的中间结果 | [运行记录协议](docs/RUNTIME_RECORDS.md) |
| 自己编译、开发或了解技术边界 | [技术与开发说明](docs/TECHNICAL_GUIDE.md) · [Windows 构建说明](docs/WINDOWS_RELEASE.md) |
| 报告问题或建议 | [GitHub Issues](https://github.com/guyucode/CodeTrail/issues)（请勿提交密钥或私有源码） |

## 使用许可

**非商业用途免费，商业用途需书面授权。** 自有代码采用 [PolyForm Noncommercial 1.0.0](LICENSE)，具体范围以许可原文为准；企业内部商业使用不能自行视为免费。[申请商业授权](COMMERCIAL-LICENSE.md)

这是源码可见项目；第三方组件保留各自许可，见 [第三方声明](THIRD_PARTY_NOTICES.md)。
