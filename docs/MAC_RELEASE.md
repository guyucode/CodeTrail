# Mac Apple Silicon 0.3.1 预览版

下载 `CodeTrail_0.3.1_aarch64.dmg`，打开后把 CodeTrail 拖入“应用程序”。已有安装可退出旧版后替换应用；项目 `.reader/` 与应用私有数据位于应用包之外，更新不清理这些数据，建议先备份项目。

配套 `CodeTrail-tools-0.3.1-macos-arm64.tar.gz` 包含同版本 reader-cli、Python SDK、协议文档和第三方许可。SHA256SUMS-mac.txt 提供附件校验。

本版包含可信 IPv4 局域网共读、共享批注、任务整合与耦合分工，以及 Git 批量读取优化。共享需要相同代码和功能关联版本；个人批注与人工阅读确认保持独立。局域网为明文 HTTP，主持电脑需保持应用运行，不提供公网穿透。

## 验证范围

发布在 Apple Silicon / macOS 27 上构建与验证。发布流程运行 Rust/CLI、Clippy、前端、Python SDK、功能关联及概览校验、统一 validate，构建生产应用与 CLI，并核验 DMG、工具包、架构、许可和 SHA-256。最终结果与原生界面检查记录见对应 GitHub Release。

不声称 Intel Mac、旧版 macOS 或两台物理电脑局域网互联已验收。本地临时签名用于验证应用完整性；没有 Apple Developer ID 签名或公证，不代表 Gatekeeper 信任。遇到系统提示应核对来源与摘要并按 macOS 正常提示处理，不关闭安全保护。

基本阅读无需 Rust、npm、模型或 API Key；Git 差异需要 Git，可选 Pyright 增强需要 Node.js 22.12+。自有代码使用 PolyForm Noncommercial 1.0.0，第三方保留各自许可。
