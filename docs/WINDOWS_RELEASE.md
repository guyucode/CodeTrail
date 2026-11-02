# Windows x64 0.3.2 预览版

本版增加可信局域网共读、共享批注及任务整合，并优化 Git 批量读取、隐藏后台 Git/Node 控制台。两台物理设备联机和 Windows 10/11 人工验收仍待完成。

从 GitHub Releases 下载 `CodeTrail_0.3.2_windows_x64_setup.exe` 安装器；配套工具为 `CodeTrail-tools-0.3.2-windows-x64.zip`，含 reader-cli.exe、Python SDK、协议文档、许可和第三方源码。下载后用 SHA256SUMS-windows.txt 核对。

基本阅读不需要 Rust、npm、模型或 API Key。Git 差异功能需要 Git；可选 Pyright 语义增强需要 Node.js 22.12+。首次安装如缺少 Microsoft WebView2，安装器从微软下载并安装该运行时，需要联网。

本版没有 Windows 代码签名证书，Windows 可能显示未知发布者或安全提示。不要关闭系统安全保护；受管理设备请按组织要求处理。暂不提供 Windows ARM、32 位或离线运行时安装包。

## 可复现构建和检查

GitHub Actions 的 Windows preview 工作流仅手动触发，使用 Windows Server 2022 x64 构建机。它执行：

1. 安装锁定的 npm 依赖，构建 CLI，校验功能证据和项目概览。
2. 通过统一 agent validate 运行 Rust/CLI、Clippy、前端、SDK、桌面编译和差异检查。
3. 根据 Windows 依赖图收集原始许可证，补齐第三方声明及 MPL 源码；缺失文本会阻止打包。
4. 构建 NSIS 安装器、release CLI 和工具 ZIP，生成 SHA-256。
5. 在隔离目录静默安装，检查安装文件、架构、许可和 Pyright 资源；启动原生程序，核验 WebView2 演示内容的辅助功能树并生成截图。

以上仅在对应工作流成功时才构成实际验证。Windows Server 构建机测试不等于 Windows 10/11 实机人工验收；首次预览版会在 Release 中列出实际工作流和验证范围。

## 本机 Windows 开发

准备 Node.js 24、Rust stable MSVC、Git、Visual Studio C++ Build Tools 和 WebView2。然后：

```powershell
npm ci
cargo test --workspace --locked
npm test
npm run desktop:build -- --bundles nsis --config src-tauri/tauri.windows.conf.json
```

发布仍使用 PolyForm Noncommercial 1.0.0，商业授权说明见 COMMERCIAL-LICENSE.md。第三方组件保留各自许可。不要将用户批注、运行数据或项目源码放进安装包。

## 0.3.2 运行记录简化

默认只显示验证目标、输入、预期、实际结果与最多5个关键中间值，详细步骤和代码按需展开。旧运行记录保持原样；未记录预期时不推断通过。Agent 可通过 validate 的可选 record 配置记录真实命令执行结果，业务变量仍须显式采集。请同步更新 reader-cli / SDK 与桌面版。
