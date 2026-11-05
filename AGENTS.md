# AGENTS.md

给编码 agent 的工作说明。人看的部分在 `README.md`，产品/架构在设计文档里（见 §11）。
**当前状态（2026-11-05 核对）：M0–M4 主体已落地，M5 Linux 客户端进行中**。
macOS 已有分屏、回看、zoom、agent 侧栏、SSH 三档对接、Metal 渲染、增量更新、选择复制粘贴、鼠标上报与
`NSTextInputClient` 输入法、Kitty keyboard protocol、编程连字；Linux 已有 GTK4/OpenGL 网格、本地宿主连接、分屏/分隔线拖动与 zoom、增量更新、IME、滚动回看、可见屏幕选择复制与异步粘贴，已接握手/静默超时与有界传输，本地自动重连、同实例增量续传与全量退化已接，尚未功能对齐。Linux 已接 Harp 风格单行侧栏、浅深色终端卡片与顶部栏，界面验收见 `docs/linux-harp.md`。
2026-11-05 核对：宿主已实现同 pane 完整状态快照与独立修订号；macOS 与 Linux 均已协商并提供 Status Records 逐条查看/确认，Linux 已接本地多会话侧栏；跨重启持久化仅 macOS 已接，见 `docs/status-sync.md`、`docs/status-client.md`、`docs/linux-sidebar-progress.md`。
2026-11-05 核对：OSC 8 已经核心、宿主帧与双外形接口传递；macOS 已接 hover/Cmd+click（仅 http/https，AppKit CI 通过，真实 GUI 待验），Linux Ctrl+click/hover 与 Kitty 键鼠/焦点上报已接，真实硬件/Wayland 待验，见 `docs/osc8.md`、`docs/linux-modern-input.md`。
2026-11-05：核心/宿主提供 generation 绑定的 literal 搜索、范围分页与原子视口定位；macOS 搜索条/分页/取消/上下命中/高亮已接入，6489ca6 的 AppKit CI 已通过，真实 GUI 待验；Linux Ctrl+Shift+F 搜索/定位已接并通过隔离 GTK 验收；regex、完整选择和稳定锚点待做，见 `docs/text-query.md`、`docs/search-progress.md`、`docs/linux-search.md`。
当前缺口以 `docs/plan.md` §11 为准；`docs/m*-checklist.md` 是各阶段的历史验收证据，不代表当前全功能已验收。

## 1. 项目是什么

pigeon 是一个**跨平台**终端（macOS 首发，Linux 跟进），自带隐形后台会话进程与一等公民侧栏：
一个窗口里左边是按项目分组的会话列表（带 agent 状态、未读、`needs you` 计数），
右边是该会话的终端本体。任务活过窗口关闭（`pigeon-server` 持有 PTY 与屏幕状态）。

硬性边界：

- **跨平台分层**：`core/`（Rust）必须能在 macOS 和 Linux 上编译运行；`PigeonApp`（Swift/AppKit）是平台专属，只有 macOS。
- **不依赖 tmux**，也不 shell out 到 tmux。
- **不 fork 其它终端**；VT 内核用 Rust 的 `alacritty_terminal` / `vte`（MIT / Apache-2.0），藏在自家 `Term` trait 后，只通过客户端接口暴露：C ABI（`include/pigeon_core.h`）或 Rust 的 `pigeon_core::api`（两者一一对应，见 §3）。
- **不修改用户的 shell/tmux 配置**，不安装任何东西到用户目录之外。
- 状态只来自 **OSC 7501**，不做屏幕/标题启发式识别。

## 2. 环境与命令

macOS + Xcode 命令行工具（`swift --version` ≥ 6）与 Rust（见 `rust-toolchain.toml`）。

```sh
cargo build --manifest-path core/Cargo.toml    # 内核 + pigeon-server（两平台都要能过）
cargo test  --manifest-path core/Cargo.toml    # 内核单测 + C ABI + 宿主端到端 + Rust 客户端 API
cargo run   --manifest-path core/Cargo.toml --bin pigeon-server   # 会话宿主（本机自动拉起；远端按需推送；手动跑用于调试）
cargo build                     # 先建核心：客户端链接 core/target/*/libpigeon_core.a
swift build                     # 客户端（macOS only）
swift test                      # 客户端纯逻辑单测（SidebarModel / Grouping / KeyEncoder）
cargo build -p pigeon-linux     # Linux 客户端（需要 GTK4；不在根的 default-members 里）
cargo test  -p pigeon-linux     # Linux 客户端纯逻辑单测（KeyEncoder / 调色板 / 实例布局）
tools/vt-parity/run.sh          # 8 条 VT 语料与已提交的 tmux golden 对账；--record 才需要 tmux
```

新增功能须先确定模块归属，不要为了跑通而把逻辑塞进 `main.rs` 或 `PigeonApp.swift`。
宿主为新 pane 固定声明 `TERM=xterm-256color` 与 `COLORTERM=truecolor`；`new_session.env` 覆盖普通环境变量并由分屏继承，不允许覆盖这两个能力变量或 `PI_PROGRAM_STATUS`，见 `docs/protocol.md`；真实 PTY 测试使用独立 HOME/配置目录，不继承用户 rc。`tools/checks/swift-logic.sh` 可在有 Swift 的 Linux/macOS 上复跑侧栏和选择纯逻辑，但不替代 macOS AppKit 验收。
`tools/bench/host-bench.py --out /tmp/pigeon-bench-result` 记录隔离 release 宿主的协议回显、快照重连和短时输出样本，不代表 GUI 显示延迟；见 `docs/host-benchmark.md`。
`swift test` 需要完整 Xcode（`xcode-select` 指向 CommandLineTools 时用 `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`）。

## 3. 模块划分（强制）

依赖方向只有一条：`客户端 → 客户端接口 → core`。Swift 客户端走 `PigeonApp → PigeonCore（C ABI） → core`；
Rust 客户端走 `pigeon_core::api`。客户端不认识 `core` 的内部类型（`term`/`layout`/`status`/`session`/`proto` 的内部结构），`core` 不知道 UI 存在。

客户端接口只有一份语义、两种外形：

- `core/src/api/` —— **安全的 Rust 客户端 API**：pane / 快照 / 状态 / 布局 / 消息编解码，
  与 C ABI 的函数分组一一对应，用所有权和 `Result<T, PgError>` 表达，不出现裸指针。
- `core/src/ffi/` —— C ABI，是 `api` 的**薄包装**：只做指针/长度转换、错误码、`catch_unwind`，不实现语义。
  `ffi` 只许调 `api`，不直接碰内部模块，保证两种外形不分叉。
- `pigeon-server` 与 `core/tests/` 属于内核自身，可以用内部模块（`internals` feature，默认开）；
  **客户端 crate 以 `default-features = false` 依赖 `pigeon-core`**，此时 `term`/`layout`/`status`/`session`/`proto`
  是私有模块，编译期只看得到 `api` 与 `ffi`。

`pigeon-server` 在 `core/` 里（同一个 crate 的 bin），所以宿主与嵌入模式跑的是同一份语义，不存在第二套实现。

可移植性标记：

- `core/`（Rust）—— **portable**：只许 std + POSIX 绑定（`libc`/`nix`/`rustix`）+ VT/序列化依赖；
  `unsafe` 只允许出现在 `core/src/ffi/`（FFI 边界），由编译器强制（见 §4）。
- `include/pigeon_core.h` —— **生成物**：由 `cbindgen` 从 `core/src/ffi/mod.rs` 生成，**手改无效**（CI 会比对新旧输出）。
- `clients/` —— **客户端层**，每个平台一个目录；客户端只通过客户端接口看图，不认识 `core` 内部类型。
- `clients/macos/` —— **macOS only**：C 头封装（`Sources/PigeonCore/`）+ AppKit 客户端（`Sources/PigeonApp/`）。
  AppKit 视图、Metal 渲染与 CoreText 字形光栅化都在这里，不要往下泄漏。
- `clients/linux/` —— M5 的 **GTK4 客户端（Rust）**：workspace 成员但**不在 `default-members`**里
  （`cargo build`/`cargo test` 在根上仍然只跑 `core`，macOS 不需要 GTK）。只依赖 `pigeon_core::api`
  （`default-features = false`）与 `docs/protocol.md`；不 import `PigeonApp`。
  网格走 `GtkGLArea` + OpenGL（ES 3 / 3.3 core 两套 shader 头），字体 `fontdb` + `swash`，
  `unsafe` 只允许在 `clients/linux/src/render/gl.rs`（GL 加载与上传边界），由 crate 级 deny 和该模块的定点放行强制；拥有型 GL 资源与隔离测试见 `docs/linux-gl-resources.md`。

```
pigeon/
├── AGENTS.md
├── GLOSSARY.md                   # 领域词汇：一个概念只有一个名字（写代码前先查这里）
├── Package.swift                 # SwiftPM：clients/macos 的三个 target（macOS only）
├── docs/                         # plan.md, protocol.md, ui.md, renderer.md, 历史资料
│   └── adr/                      # 架构决策记录：为什么是这样而不是别的那样
├── tools/                        # 开发期脚本（不进产品；各自定位仓库根，见 tools/README.md）
│   ├── build/                    # bundle-app.sh：打 Pigeon.app
│   ├── bench/                    # render-bench.sh：渲染基准
│   ├── checks/                   # 手工验收脚本 + helpers/（Swift 小工具）
│   └── vt-parity/                # VT 对账：corpus / golden / run.sh
├── include/pigeon_core.h         # 生成物：cbindgen 从 core/src/ffi/mod.rs 输出，不许手改
├── core/                         # Rust 内核 + 会话宿主（crate + bin，平台无关）
│   ├── Cargo.toml
│   ├── cbindgen.toml             # 头文件生成配置（名字/前缀/文档注释）
│   ├── src/
│   │   ├── lib.rs                # 模块声明；只有 ffi 模块放行 unsafe_code
│   │   ├── api/                  # 安全的 Rust 客户端 API（Rust 客户端唯一入口）
│   │   ├── ffi/                  # extern "C" 导出（头文件的唯一来源；api 的薄包装）
│   │   │   ├── mod.rs            # 生命周期 / pane / 快照 / 状态 / 布局
│   │   │   └── msg.rs            # pg_msg_* 消息编解码
│   │   ├── term/                 # VT：alacritty_terminal 包装 + 快照/脏行/滚动
│   │   ├── layout/               # 布局树：嵌套 vsplit/hsplit、比例、最小尺寸
│   │   ├── status/               # OSC 7501 解析与记录生命周期
│   │   ├── session/              # 会话/pane、尺寸策略与增量续传
│   │   └── proto/                # 分帧 + CBOR 编解码（协议的唯一实现）
│   ├── src/bin/pigeon-server/      # 会话宿主：main / cli / transport + 四个组（3–8MB 硬指标）
│   │   ├── host/                 # 共享状态：sessions、pane 元数据、事件总线
│   │   ├── connection/           # 一条连接：client、握手、出站队列、追赶
│   │   ├── pane/                 # 一个 pane 的任务：actor / cmd / query
│   │   └── dispatch/             # 入站消息 → 效果：dispatch + layout / pane
│   └── tests/                    # abi（C 外形）+ api（Rust 外形）+ host（真实 PTY 端到端）
└── clients/                      # 客户端：每个平台一个目录
    ├── macos/
    │   ├── Sources/
    │   │   ├── PigeonCore/       # C 头封装（module map + 类型映射，不实现语义）
    │   │   └── PigeonApp/        # macOS 客户端：只做 UI 与输入
    │   │       ├── main.swift        # 入口：SIGPIPE、NSApplication、装配 AppDelegate
    │   │       ├── PigeonApp.swift   # AppDelegate 本体：状态 + 两个生命周期钩子
    │   │       ├── AppDelegate+*.swift  # 按分节拆开的 extension：Window / Connection / Commands / Sessions / Hosts / Protocol
    │   │       ├── Net/          # 连到宿主与远端
    │   │       │   ├── Connection/   # 一条 socket：分帧、hello、心跳
    │   │       │   ├── Host/         # 一台机器：命名、持久化、安装、拉起、恢复
    │   │       │   └── Session/      # a/b/c 三档对接与退化模式
    │   │       ├── Sidebar/
    │   │       │   ├── SidebarModel.swift      # 分组/排序/未读/计数（纯逻辑，可单测）
    │   │       │   ├── SidebarViewController.swift
    │   │       │   ├── SidebarRowView.swift    # 一行：名称 + 相对时间 + 状态图标
    │   │       │   └── Grouping.swift          # 项目分组规则（git 根 / cwd 末级目录）
    │   │       ├── Terminal/
    │   │       │   ├── PaneView.swift          # 终端视图容器（焦点、IME、鼠标）
    │   │       │   ├── CoreTextRenderer.swift  # 唯一 import CoreText 的文件：字体度量 + 字形光栅化
    │   │       │   ├── PaneView+Metal.swift    # 把格子交给渲染线程（唯一的网格渲染路径）
    │   │       │   ├── Render/                 # GPU 渲染：图集、脏行、实例构建、Metal 线程与 shader
    │   │       │   └── KeyEncoder.swift        # 按键 → 字节（legacy + KittyKeyEncoder）
    │   │       ├── Layout/           # 分屏：把 PgRect 画成 AppKit 视图、焦点与选择
    │   │       └── Update/           # 软件更新：检查 latest.json、验签、替换 Pigeon.app 并重启
    │   └── Tests/PigeonAppTests/     # SidebarModel / Grouping / KeyEncoder 等纯逻辑单测
    └── linux/                    # M5：GTK4 客户端（Rust，第一片已跑通；见 clients/linux/README.md）
```

协议与宿主只存在于 `core/src/proto/` 与 `core/src/bin/pigeon-server/`，没有 Swift 版：宿主必须是 3–8MB
静态单文件（`docs/remote.md`），Swift 运行时做不到；协议只留一处实现，客户端通过 C ABI 编解码（见 `docs/protocol.md` §9）。

### 新增代码放哪里（判定表）

| 你要加的东西 | 放这里 |
|---|---|
| 一种新的 OSC/控制序列的处理 | `core/src/term/`（渲染相关）或 `core/src/status/`（状态记录）；协议字段只有需要上线时才动 `core/src/proto/` |
| 窗口与后台进程都要知道的数据 | `core/src/session/` + `core/src/proto/` + 重新生成头文件 |
| 只在 UI 里用的派生状态（排序、分组、文案） | `clients/macos/Sources/PigeonApp/Sidebar/SidebarModel.swift`（保持纯函数，方便单测） |
| 新的通信消息 | `core/src/proto/` 的 message enum + `pigeon-server` 的处理分支 + `core/tests/proto` 往返 |
| 终端内核能力（换引擎 / 接别的内核） | `core/src/term/`（`Term` trait），C ABI、客户端、协议都不动 |
| 渲染、字体、输入法 | `clients/macos/Sources/PigeonApp/Terminal/`，不往下漏 |
| 一次性脚本 / 对账工具 | `tools/` 下的 `build/` `bench/` `checks/` `vt-parity/`，不放进 `clients/` 或 `core/src/` |

### 可移植性硬规则（CI 跑 fmt / clippy（含 §4 的 lint）/ test / tokio+portable-pty 引用检查 / `--lib --no-default-features` 构建 / 体积上限 / 头文件 diff；直接依赖与平台 import 有静态检查）

- `core/` 里**不许**出现：平台专属 crate（`core-foundation`、`objc2`…）、`cfg(target_os = …)` 分支
 （PTY 与信号可以有一处，且必须两边都有实现）、直接操作窗口/字体/图形 API。
- `core/src/` 里**不许**出现 `unsafe`（唯一例外：`ffi/` 的 `extern "C"` 边界）。`core/Cargo.toml` 的 `[lints]` 把
  `unsafe_code` 设成 deny，只有 `lib.rs` 里的 `ffi` 模块放行；宿主需要的 POSIX 调用走 `nix`，不自己写 `unsafe`。
- Swift 侧：`PigeonCore` 不许 `import AppKit`；`PigeonApp` 可以，但 `import CoreText` 只允许在
 `CoreTextRenderer.swift`。
- PTY 用 `portable-pty`（内部就是 `forkpty`/`openpty` 的封装）。因为 `core/src` 除 `ffi/` 外不许 `unsafe`，
  **不直接操作原始 fd**；也不用 `NSTask`/`Process` 之类 Apple 专属机制。
- 路径用 `FileManager`/POSIX；配置放 `~/.config/pigeon`（macOS 与 Linux 一致），不要用 `~/Library/...`。
- 信号（`SIGWINCH`/`SIGCHLD`）用 `signal()` 或 `SelfPipe`/`signalfd` 的思路，不依赖 macOS runloop 特性。
- 字体/渲染/输入法只在客户端层处理；`core/` 只搬字节与状态。
- `tokio` / `portable-pty` 是 **optional 依赖，只由 `host` feature 打开（默认开），且只允许在 `core/src/bin/` 里使用**。
  CI 会跑 `cargo build --lib --no-default-features`，并 grep 保证库本体不引用它们。

`tools/checks/architecture.py` 校验 core 直接依赖清单、host optional feature、Rust 客户端关闭默认/internal feature、核心源码中的 host runtime/OS 分支和 Swift UI/CoreText import。新增直接依赖须按本文件评审并更新清单；文本检查不是编译器，也不审计全部传递依赖。

### 模块内的硬规则

- **一个文件一个主要类型**，文件名 = 类型名；单文件 > 400 行就拆。
- `core/src/term/` 是**唯一**实现 VT 语义的地方（包括 `alacritty_terminal` 的包装）；Swift 侧不得出现任何字节流解析。
- `core/src/status/` 是**唯一**实现 OSC 7501 解析与记录生命周期的地方。
- `core/src/proto/` 是**唯一**实现分帧与 CBOR 编解码的地方；客户端通过 C ABI 或 `pigeon_core::api` 编解，不自己实现一份。
- 客户端接口的两种外形必须同步：给 C ABI 加函数就同时加 `api` 的对应项，反之亦然；
  `core/tests/abi.rs` 测 C 外形，`api` 的用例测 Rust 外形。
- `include/pigeon_core.h` 是**生成物**：改 ABI 就去改 `core/src/ffi/mod.rs`，然后跑 `cbindgen`，不要手改头文件。
- `SidebarModel` / `Grouping` / `KeyEncoder` 必须是**纯逻辑**（无 AppKit、无 IO），可直接单测。
- `pigeon-server` 里不能出现任何 UI 概念；`PigeonApp` 里不能出现 PTY 操作。
- 任何跨进程行为都通过 `core/src/proto/` 的显式消息表达；不要用共享全局状态。
- **终端语义只能在内核里实现**：VT 解析、快照/增量、布局计算、OSC 7501 记录都不许在客户端重复实现；
 客户端只做绘制与输入映射。

## 4. 代码风格

- Swift 6 工具链（语言模式 v5，见 `Package.swift`）/ Rust 2021 edition，4 空格缩进；公开 API 写显式类型；`guard`/`if let` 早返回，避免深嵌套。
- Rust：提交前 `cargo fmt` + `cargo clippy --all-targets -- -D warnings`。生产路径返回 `Result<T, PgError>`；
  `unwrap()`/`expect()`/`panic!` 由 `[lints.clippy]` 设成 deny（`core/clippy.toml` 放行单元测试，
  `core/tests/*`、`core/examples/*` 在文件头放行），每个 `unsafe` 块前必须有 `// SAFETY:` 注释（`undocumented_unsafe_blocks`）。
  FFI 导出一律包 `catch_unwind`，panic 不越过边界。不要用 `#[allow]` 绕过这些 lint；确需例外，在 PR 里说明。
- **后台进程里禁止 `!` 强解包 / `try!`**；所有失败返回 `Result` 或 `throws` 的自定义 error enum。
- 并发：`core/` 不用可变全局；每个 pane 的状态由宿主串行驱（一个 task/线程一处所有权）；`App` 的 UI 状态只在主线程改。
- 命名：类型 UpperCamelCase，函数/变量 lowerCamelCase；协议名用名词（`Term`、`GridRenderer`）而不是 `-able` 广告。
- 注释只写“为什么”，不写“做了什么”；对外部协议的实现要在文件头注明依据（例如 OSC 7501 revision）。
- 语言约定：**代码与注释英文；`docs/` 中文；UI 文案英文**（与参考图一致，如 `Working · 1m`）。
- 文档只写现状：功能删了就删掉对应文字，改名就按新名字重写，不留“上一版…”“已取消”“做过又删”之类的历史说明（历史在 git 里）。

## 5. 测试要求

- 新增解析规则（OSC 7501、按键编码、协议消息）→ **必须**带一个正例 + 一个畸形输入的反例（Rust 放 `core/tests/`，
  Swift 放 `Tests/`）。
- 修 bug → 先写能复现的测试，再修。
- VT 兼容性改动 → 跑 `tools/vt-parity/run.sh`（已提交的 8 条文本 golden）与相关 `core/tests` 确定性用例；文本对账不能替代属性、输入及渲染验收。
- 提交前必须全绿：`cargo test`（macOS + Ubuntu）+ `swift build && swift test`（macOS）。
- 测试不要依赖网络、不要依赖用户自己的 tmux/shell 配置；需要环境时自己在 `TMPDIR` 里造。

## 6. 常见任务的做法

**程序主动上报状态**：`tools/report/pigeon-report` 与可选 `pigeon-run`，安装、字段和退出语义见 `docs/agent-integration.md`；不自动安装 hook 或修改用户配置，不把能力环境变量当成接入成功。

**加一种状态显示**：`core/src/status/` 已有记录 → 只改 `SidebarModel` 的排序/图标映射 + `SidebarRowView`，不动协议。

**加一条通信消息**：`core/src/proto/` 的 message enum 加 case（带 version）→ 分帧/编解码自动覆盖 →
`pigeon-server` 加处理分支 → `core/tests/proto` 加编解码往返 → 重新生成 `include/pigeon_core.h` → 客户端订阅处处理。

**换 VT 引擎**：只替换 `core/src/term/` 的实现（`Term` trait），C ABI、客户端、协议都不动。

**加一个侧栏字段（如“最后活动时间”）**：`core/src/session/` 加字段 → `core/src/proto/` 带过去 →
重新生成头文件 → `SidebarModel` 用 → 测试。

## 7. 不变量（谁都不许破）

1. 关掉客户端 / 客户端崩溃，pane 里的进程必须继续跑（`pigeon-server` 持有 PTY）。
2. 重连后的屏幕必须与断开前一致（含备用屏、光标、滚动区）——用全量快照 + 增量，不靠重放历史字节。
3. OSC 7501 的丢弃规则严格按规范：超长、解不出 base64、解码后含控制字符 → **整条丢弃**，不能只丢一半。
4. `done` / `error` 记录在用户看过之前不能被静默清掉；`working` / `blocked` 在进程退出或 OSC 133A 提示符时必须清掉。
5. 不可信输入（程序写的 OSC 文本、链接、剪贴板）不得进入 UI 的富文本渲染路径；永远当纯文本处理。
6. 多客户端同看一个会话时，尺寸策略必须显式且可见（默认跟随最近活跃客户端，并在侧栏标出）。
7. `core/` 不得引入 Apple-only 依赖；`cargo test` 在 Ubuntu 红了就等于功能没做完。

## 8. 安全

- 一切来自 pane 的字节都是不可信的：`msg`/`title` 只当纯文本；拒绝控制字符；不解析成链接之外的标记。
- OSC 52 的实现必须默认只写不读；读需要用户显式许可。当前读写事件均未接通，不要把安全要求写成已支持能力。
- 后台进程 socket 权限 0700，只允许当前用户；不接受网络监听。
- 不要为了“方便调试”打开任意命令执行入口（例如调试端口跑 shell）。

## 9. 许可证与依赖

- 本项目 **GPL-3.0-or-later**：新文件不需要逐个头注释，但改动视为以 GPLv3 授权；新增依赖必须与 GPLv3 兼容
  （MIT / Apache-2.0 / BSD / ISC 可用；LGPL 仅限动态链接；**禁止** GPL 不兼容的许可证，例如把 CDDL/EPL 代码静态链接进来）。
- 引用/改编其它终端（kitty、tmux、Alacritty、Ghostty…）的代码时：只能借鉴思路与规格；**逐行复制需要同许可证**，
  且要在文件头注明来源与许可证。纯规格（例如 OSC 7501 文档、tmux 的协议行为）不受此限。
- `Cargo.toml` / `Package.swift` 建起来时立刻写上 `license = "GPL-3.0-or-later"`（Rust）/ 对应 Swift 元数据。

## 10. 提交与 PR

- 提交信息：`<模块>: <一句话>`，例如 `sidebar: 把 blocked 会话排到组内最前`。
- 一个提交只做一件事；重构与功能分开。
- PR 描述包含：动机、改动面（哪些模块）、验证方式（命令 + 结果）、已知缺口。
- 合并前：`cargo test && swift build && swift test` 全绿；涉及 VT 的改动附对账结果。

### 文档同步（强制）

**文档必须持续反映当前实现；更新相关文档是完成任务的一部分。**

- 改功能、行为、协议、配置、命令、目录结构或支持平台时，必须在同一项任务中更新对应文档；不要留到以后补。
- 交付前核对 `README.md`、本文件的当前状态、`docs/plan.md` 的已知缺口，以及受影响的专题文档；已完成项从待办/缺口移除，新增限制如实记录。
- 明确区分**当前已实现、已验证、计划中、未验证**；以代码和实际验证为依据，不把规划、代码存在或旧测试通过写成当前端到端已通过。
- 当前能力与缺口集中维护在 `docs/plan.md` §11，平台细节放专题文档；其它页面优先链接，避免多份状态清单漂移。核对现状时注明日期。
- `docs/m*-checklist.md`、旧方案与上游对照属于历史证据：保留当时的测试数字、日志、截图和提交信息，标注适用阶段/日期并链接当前文档；不要改写成今天重新验收过。
- 发现与本次任务相关的过时或互相矛盾的描述，顺手修正并检查相对链接。生成物仍由生成工具更新，不能手改。
- 纯文档改动至少运行文档链接检查与 `git diff --check`；涉及功能的改动仍须执行该功能要求的测试。交付时说明更新了哪些文档及未验证的部分。

## 11. 设计文档在哪

- `GLOSSARY.md` —— 领域词汇表：一个概念只有一个名字，含「禁用的说法」（**写代码前先读**）
- `docs/adr/` —— 架构决策记录（ADR）：已定完、别再重新讨论的事，以及否掉过的选项（**改设计前先读**）
- `docs/plan.md` —— 架构、选型、里程碑、验收、风险、已知缺口（**先读这个**）
- `docs/terminal-gap-analysis.md` —— 2026-11-04 与代表性终端的能力对照、证据与建议优先级（日期性评估，不替代当前缺口表）
- `docs/optimization-plan.md` —— 当前优化执行计划：阶段依赖、主力/前沿版本门槛、首批任务；实施后同步勾选，现状仍更新到 `docs/plan.md` §11
- `docs/core.md` —— 内核边界、C ABI 说明、快照/增量、语言选型、测试策略
- `docs/protocol.md` —— 客户端 ⇄ 宿主的字段级契约（消息表、错误码、能力、尺寸策略）
- `include/pigeon_core.h` —— C ABI 的生成产物（唯一对外的 C ABI 面；Rust 客户端对应 `pigeon_core::api`）
- `docs/remote.md` —— SSH 方案：远端宿主、三种对接、安装与安全
- `docs/update.md` —— 软件更新：检查、签名校验、替换与重启、发版步骤
- `docs/gui.md` —— 前端路线：为何 AppKit/GTK、渲染与 IME 的归属
- `docs/ui.md` —— 侧栏与窗口的可见规格（含参考图，`docs/images/`）
- `docs/tmux-integration.md`、`docs/kitty-fork-status.md` —— 上一版（gmux）的实测记录，当反面教材读
