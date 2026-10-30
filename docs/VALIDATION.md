# Mac 发布验证

本次 Apple Silicon Mac Preview 的实际验证结果见 RELEASE_VALIDATION.md。下列为历史 Linux 验收记录，不代表当前版本测试数量或 Mac 安装包结果。

# CodeTrail v0.2 验收记录

本记录对应 v0.2 交付源码的实际验证结果。开发与原生构建环境为 Linux x86_64；macOS 和 Windows 尚未执行原生验收。产品中的“已审阅”表示用户对所绑定代码快照的人工阅读状态，不代表程序正确性已经得到测试证明。

## 验收结果

| 检查 | 结果 | 验证内容 |
| --- | --- | --- |
| `cargo test --workspace --locked` | **67 个测试通过** | 23 个单元测试：9 个 Git、1 个关系快照校验、13 个语义适配；22 个原有索引/锚点/审阅行为集成测试；22 个关系分析集成测试 |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | 通过 | 核心 crate 与 CLI，无 lint 警告 |
| `npm test` | **7 个测试通过** | 演示标注持久化、审阅快照确认、空功能限制、直接改动关联、真实示例关系、动态/历史边界，以及旧演示标注兼容 |
| `npx tsc --noEmit` | 通过 | 前端类型检查 |
| `npm run build` | 通过 | TypeScript 检查、Vite 生产构建、本地 Monaco workers |
| `npm run test:ui` | **21 项通过，pageerror 为 0** | 浏览器中的标注与审阅、Diff、关系图、调用/引用证据、功能影响和导航交互 |
| 关系界面视觉复核 | 通过 | 1440 px 与 1000 px 视口下，关系图和证据区域未出现水平溢出 |
| `npm run tauri -- build --debug --no-bundle` | 通过 | 最终 Linux 原生 Tauri 可执行文件构建；此命令不生成安装包 |
| `npm run test:native:managed` | **40 项断言通过，JavaScript 错误为 0** | 实际目录选择、WebKit/Monaco、实时 Rust 关系、真正 Pyright、过期证据导航拒绝及原有 IPC 行为 |

浏览器 smoke 使用随源码提供的演示数据验证界面；Rust 测试使用临时源码和 Git 仓库验证实际解析与关系行为。语义测试同时覆盖受控 LSP 响应和真正的 Pyright。原生 smoke 在真实 WebKit 窗口中打开临时仓库，并通过 Tauri IPC 完成静态与语义关系验证；最终 40 项断言全部通过，源码总哈希保持不变。此交付不包含内部验收截图。

## 有实际回归测试的关键行为

### 索引、锚点与审阅

- 打开仓库不会创建 `.reader/`；标注操作才写入元数据。
- 支持语言的声明与范围来自 tree-sitter；不是正则表达式猜测函数边界。
- 名称和内容不变的函数在同一文件中移动后，锚点可以重新定位；内容变动使已确认状态失效。
- 已绑定函数重命名后，旧锚点仍可见并显示失效；不会静默消失或绑定到另一函数。
- 部分行范围只在原所属符号内按唯一内容重新定位，防止迁移到其他函数的相同片段。
- 界面读到快照后源码变化，旧摘要的确认请求会被拒绝；重新读取并人工确认才可更新状态。
- 坏 JSON、未来版本的元数据、循环父子关系和失效编辑目标不会被静默覆盖。
- 越界路径、内部目录路径、符号链接的读写范围扩张受到限制。
- 创建、绑定、审阅、导出及删除功能不会改写源文件。

### Git 差异与关系影响

- Git 对比涵盖已暂存、未暂存、未跟踪文件、删除、已暂存重命名以及两个历史提交。
- 删除函数使用旧版本符号归属判断，不把旁边未改动的函数误判为变更；阅读工作区 Diff 不执行仓库配置的 clean filter。
- 对 `D → A → B → C`，当 B 改动时，B 对应功能标记为直接改动，A 对应功能可获得一跳“可能受影响”证据；C、D 和仅持有 B 函数引用的功能不据此扩散影响。
- 同一功能同时具备直接差异与依赖证据时，直接改动优先。“可能受影响”不改写已有人工审阅状态。
- 每条已解析关系包含实际调用/读取位置、定义范围和来源。位置列采用 1-based UTF-16；同一行的多个调用具有不同且可重复生成的关系 ID。
- 关系快照绑定文件内容哈希；分析期间文件修改、新增或删除时，过期结果会被拒绝。界面导航还会核对目标文件当前哈希，拒绝使用已过期的证据位置。
- 图只表示当前 WORKTREE。历史比较不会使用当前关系推断旧版本影响；已删除或重命名旧身份的历史调用者明确为未知。

### Python 解析与语义边界

- 精确模块路径、导入别名、相对导入、重导出和同名不同模块均有回归；多个源码根对应同一模块名时不会任选一个目标。
- 参数、赋值、闭包、类作用域、条件重复绑定、通配符导入、推导式和模式绑定不会通过全局同名搜索误连。
- 保存、返回或传递函数值属于引用，不等同于调用。默认参数和装饰器表达式不会冒充被声明函数体的调用；匿名或延迟执行域不会被直接归到外层函数。
- `factory()()` 的外层返回值调用、下标调用和条件调用不会因为共享标识符位置，被 LSP 误升级为该名称的直接调用。语义补全核验捕获 AST 形态与关系身份。
- 已观察到的模块属性写入会按实际绑定 owner 和导入模块路径保守失效，覆盖跨函数和不同导入别名；同名局部参数不会污染另一个全局导入。definition 和 references 均不能恢复已失效的原目标。
- 带装饰器的声明及相关边会提示：装饰器可能替换可调用对象，已解析关系仅绑定源码声明，不保证实际运行时目标。
- LSP 返回参数或局部变量位置时，不会误映射成包含它的函数；多目标和外部目标保持明确状态，references 不覆盖 definition 的歧义结论。
- Pyright 适配验证了真实定义/引用、请求能力、路径与 URI、消息预算、异常输出、超时退出和进程权限边界。语言服务不可用时保留静态关系。

关系回归验证的是上述可观察边界，不是完整 Python 运行时模拟。参数或容器传递、一般赋值别名、反射和未配置的导入路径仍可能形成未解析关系；模块属性写入采用整体保守失效，也不证明该写入实际执行。其他语言继续提供既有声明与 Diff 阅读，但 v0.2 不推断其调用关系。

## 如何复跑

### 核心与前端

在源码根目录、已安装所需 Node.js/Rust/Git 的环境执行：

```bash
npm ci
cargo test --workspace --locked
cargo clippy --workspace --all-targets --locked -- -D warnings
npm test
npx tsc --noEmit
npm run build
```

`npm ci` 安装锁定版本的 Pyright。真正的语义用例及原生语义验收需要可用的 Node.js 与应用依赖；仅静态关系分析不需要启动语言服务器。可先运行 `npm run doctor` 检查开发环境。

### 浏览器交互

先安装 Playwright 浏览器，并在一个终端启动开发服务：

```bash
npx playwright install chromium
npm run dev
```

在另一个终端运行：

```bash
npm run test:ui
```

测试默认连接 `http://127.0.0.1:1420`，可通过 `BASE_URL` 覆盖。每次使用独立浏览器上下文，演示标注不会污染已有浏览器会话。脚本断言实际 Monaco Diff、只读输入及关系导航的界面行为。

### Linux 原生集成：托管启动

先安装 README 中的 Tauri Linux 原生开发依赖。目录选择测试还需要 `webkit2gtk-driver`、`xdotool` 和 `tauri-driver`；无图形桌面时需要 Xvfb、Xauthority 与 D-Bus 工具。例如在 Debian/Ubuntu 环境：

```bash
sudo apt-get install webkit2gtk-driver xdotool xvfb xauth dbus-x11
cargo install tauri-driver --locked
npm run tauri -- build --debug --no-bundle
xvfb-run -a dbus-run-session -- npm run test:native:managed
```

托管脚本启动驱动、等待就绪、运行 smoke，并在结束后关闭驱动。**驱动、测试脚本和操作原生目录对话框的 `xdotool` 继承同一个 `DISPLAY`**。在已有 X11 桌面会话中，也可直接执行 `npm run test:native:managed`。

扩展原生脚本创建并清理独立临时 Git 仓库，不使用用户项目。它通过真实桌面按钮打开原生目录选择框并选择该仓库；随后检查实时 Rust 解析的跨文件调用者、函数值引用和功能影响，再通过实际 Tauri IPC 请求真正的 Pyright。脚本还会临时修改测试源码，验证旧证据位置被界面拒绝、刷新后恢复导航，以及旧审阅摘要不能确认新内容。测试模拟的源码改动会恢复，阅读器操作前后的源码总哈希必须一致。

可配置的运行选项：

| 环境变量 | 默认值 | 用途 |
| --- | --- | --- |
| `TAURI_DRIVER` | `tauri-driver` | 托管脚本启动的驱动可执行文件，可指定绝对路径 |
| `CODETRAIL_TEST_PORT` | `4444` | 托管驱动端口，原生驱动使用下一端口；范围为 1024–65533，需预留两个端口 |
| `TAURI_APPLICATION` | `src-tauri/target/debug/codetrail-desktop` | 被测试的原生可执行文件，建议覆盖时使用绝对路径 |
| `WEBDRIVER_URL` | `http://127.0.0.1:4444` | 手动运行 smoke 时使用的现有驱动地址；托管脚本按所选端口设置此值 |

例如，使用另一个端口和已安装驱动路径：

```bash
TAURI_DRIVER="$HOME/.cargo/bin/tauri-driver" CODETRAIL_TEST_PORT=4454 xvfb-run -a dbus-run-session -- npm run test:native:managed
```

### Linux 原生集成：复用已有驱动

已有驱动时可以手动运行：

```bash
tauri-driver --port 4444 --native-port 4445
```

在**同一 X11 会话**的另一个终端执行：

```bash
WEBDRIVER_URL=http://127.0.0.1:4444 npm run test:native
```

两个终端必须使用相同的 `DISPLAY`；设置了 `XAUTHORITY` 时，也必须使用同一个授权文件。若驱动位于 Xvfb 内，测试进程也必须继承该 Xvfb 的 `DISPLAY` 和 `XAUTHORITY`。只给驱动套一层 `xvfb-run`、随后从另一个显示环境运行测试，无法操作它的原生目录选择框；无头环境优先采用上面的托管命令。

## 尚未验证的事项

- macOS arm64 的原生运行、签名、公证与 DMG 安装。
- Windows 原生安装包与 WebView2 运行。
- 超大仓库的吞吐和长时间稳定性；当前版本面向小型到中型阅读任务，设有扫描与语义分析预算。
- 多个桌面进程同时修改同一个 `.reader/` 的协同写入。当前写入在单进程内串行，并使用原子文件替换；不提供跨进程事务。

发布桌面安装包前，应在目标平台重复真实仓库流程并完成平台构建。当前交付是源码版本和已构建的浏览器演示资源；Linux 的 `--no-bundle` 构建通过不等于安装包验收。交付不包含已签名、公证的 macOS 应用或已验证的 Windows 安装包。
