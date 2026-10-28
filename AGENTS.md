# AGENTS.md

给编码 agent 的工作说明。人看的部分在 `README.md`，产品/架构在设计文档里（见 §11）。
**当前状态：M3 已落地**——`core/`（VT / OSC 7501 / 协议 / 布局 / C ABI）、`pigeon-host`（多 pane、状态流）、
AppKit 客户端（分屏、侧栏、未读/needs-you）；远端/IME 仍在后续里程碑。

## 1. 项目是什么

pigeon 是一个**跨平台**终端（macOS 首发，Linux 跟进），自带隐形后台会话进程与一等公民侧栏：
一个窗口里左边是按项目分组的会话列表（带 agent 状态、未读、`needs you` 计数），
右边是该会话的终端本体。任务活过窗口关闭（`pigeon-host` 持有 PTY 与屏幕状态）。

硬性边界：

- **跨平台分层**：`core/`（Rust）必须能在 macOS 和 Linux 上编译运行；`PigeonApp`（Swift/AppKit）是平台专属，只有 macOS。
- **不依赖 tmux**，也不 shell out 到 tmux。
- **不 fork 其它终端**；VT 内核用 Rust 的 `alacritty_terminal` / `vte`（MIT / Apache-2.0），藏在自家 `Term` trait 后，只通过 C ABI（`include/pigeon_core.h`）暴露。
- **不修改用户的 shell/tmux 配置**，不安装任何东西到用户目录之外。
- 状态只来自 **OSC 7501**，不做屏幕/标题启发式识别。

## 2. 环境与命令

macOS + Xcode 命令行工具（`swift --version` ≥ 6）与 Rust（见 `rust-toolchain.toml`）。

```sh
cargo build --manifest-path core/Cargo.toml    # 内核 + pigeon-host（两平台都要能过）
cargo test  --manifest-path core/Cargo.toml    # 内核单测 + ABI + 宿主端到端（64 个用例）
cargo run   --manifest-path core/Cargo.toml --bin pigeon-host   # 会话宿主（本机自动拉起；远端按需推送；手动跑用于调试）
cargo build                     # 先建核心：客户端链接 core/target/*/libpigeon_core.a
swift build                     # 客户端（macOS only）
swift test                      # 客户端纯逻辑单测（SidebarModel / Grouping / KeyEncoder）
# tools/vt-parity/run.sh        # VT 对账（M1 建；现在还只有 core/tests 的确定性用例）
```

这些命令在对应模块落地前会失败——**先建模块，再加功能**，不要为了跑通而把逻辑塞进 `main.rs` 或 `PigeonApp.swift`。
`swift test` 需要完整 Xcode（`xcode-select` 指向 CommandLineTools 时用 `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`）。

## 3. 模块划分（强制）

依赖方向只有一条：`App → PigeonCore（C ABI） → core`。`App` 不认识 `core` 的内部类型，`core` 不知道 UI 存在。
`pigeon-host` 在 `core/` 里（同一个 crate 的 bin），所以宿主与嵌入模式跑的是同一份语义，不存在第二套实现。

可移植性标记：

- `core/`（Rust）—— **portable**：只许 std + POSIX 绑定（`libc`/`nix`/`rustix`）+ VT/序列化依赖；
  `unsafe` 只允许出现在 `core/src/ffi/`（FFI 边界）。
- `include/pigeon_core.h` —— **生成物**：由 `cbindgen` 从 `core/src/ffi/mod.rs` 生成，**手改无效**（CI 会比对新旧输出）。
- `Sources/PigeonCore/` —— **macOS only**：C 头的 module map + 类型映射，薄到没有语义。
- `Sources/PigeonApp/` —— **macOS only**：AppKit 视图与 CoreText 渲染都在这里；不要往下泄漏。
- 将来的 Linux 客户端是**另一个 target**（例如 `Sources/PigeonAppLinux`，GTK4），只依赖 C ABI 与
  `docs/protocol.md`，不 import `PigeonApp`。

```
pigeon/
├── AGENTS.md
├── Package.swift                 # targets: PigeonApp / PigeonCore / PigeonAppTests（macOS only）
├── docs/                         # plan.md, protocol.md, ui.md, 历史资料
├── tools/vt-parity/              # M1：对账脚本与 golden 文件（不含产品代码）
├── include/pigeon_core.h         # 生成物：cbindgen 从 core/src/ffi/mod.rs 输出，不许手改
├── core/                         # Rust 内核 + 会话宿主（crate + bin，平台无关）
│   ├── Cargo.toml
│   ├── cbindgen.toml             # 头文件生成配置（名字/前缀/文档注释）
│   ├── src/
│   │   ├── lib.rs                # 模块声明；unsafe 只允许出现在 ffi.rs
│   │   ├── ffi/                  # extern "C" 导出（头文件的唯一来源）
│   │   │   ├── mod.rs            # 生命周期 / pane / 快照 / 状态 / 布局
│   │   │   └── msg.rs            # pg_msg_* 消息编解码
│   │   ├── term/                 # VT：alacritty_terminal 包装 + 快照/脏行/滚动
│   │   ├── layout/               # 布局树：嵌套 vsplit/hsplit、比例、最小尺寸
│   │   ├── status/               # OSC 7501 解析与记录生命周期
│   │   ├── session/              # session/window/pane 模型
│   │   └── proto/                # 分帧 + CBOR 编解码（协议的唯一实现）
│   ├── src/bin/pigeon-host.rs    # 会话宿主：PTY / unix socket / 多客户端（3–8MB 硬指标）
│   └── tests/                    # abi（C ABI 内嵌路径）+ host（真实 PTY 端到端）
└── Sources/
    ├── PigeonCore/               # C 头封装（module map + 类型映射，不实现语义）
    └── PigeonApp/                # macOS 客户端：只做 UI 与输入
        ├── PigeonApp.swift       # @main，装配依赖
        ├── Window/               # 窗口、标题栏、红绿灯、窗口级快捷键（M3 侧栏做完再拆）
        ├── Sidebar/
        │   ├── SidebarModel.swift      # 分组/排序/未读/计数（纯逻辑，可单测）
        │   ├── SidebarViewController.swift   # M3
        │   ├── SidebarRowView.swift    # 一行：名称 + 相对时间 + 状态图标（M3）
        │   └── Grouping.swift          # 项目分组规则（git 根 / cwd 末级目录）
        ├── Terminal/
        │   ├── PaneView.swift          # 终端视图容器（焦点、IME、鼠标）
        │   ├── GridRenderer.swift      # 协议：把 PgSnapshot 画成像素（M0 直接调 CoreTextRenderer）
        │   ├── CoreTextRenderer.swift  # 唯一 import CoreText 的文件
        │   └── KeyEncoder.swift        # 按键 → 字节（kitty protocol / legacy）
        └── Layout/               # M1 分屏
            ├── SplitView.swift         # 把 PgRect 画成 AppKit 视图
            └── Focus.swift             # 焦点与选择
```

`Window/`、`Layout/`、`SidebarViewController.swift`、`SidebarRowView.swift`、`GridRenderer.swift`
是后续里程碑（M1/M3）的落点；M0 只建了 `Terminal/{PaneView,CoreTextRenderer,KeyEncoder}.swift`、
`Net/{HostClient,Wire}.swift`、`Sidebar/{SidebarModel,Grouping}.swift`。

后两个之前存在的 Swift 模块（`PigeonShared` / `PigeonHost`）已取消：协议与宿主现在只存在于 `core/src/proto/`
与 `core/src/bin/pigeon-host.rs`。理由：宿主必须是 3–8MB 静态单文件（`docs/remote.md`），Swift 运行时做不到；
而协议只留一处实现，客户端通过 C ABI 编解码（见 `docs/protocol.md` §9）。

### 新增代码放哪里（判定表）

| 你要加的东西 | 放这里 |
|---|---|
| 一种新的 OSC/控制序列的处理 | `core/src/term/`（渲染相关）或 `core/src/status/`（状态记录）；协议字段只有需要上线时才动 `core/src/proto/` |
| 窗口与后台进程都要知道的数据 | `core/src/session/` + `core/src/proto/` + 重新生成头文件 |
| 只在 UI 里用的派生状态（排序、分组、文案） | `Sources/PigeonApp/Sidebar/SidebarModel.swift`（保持纯函数，方便单测） |
| 新的通信消息 | `core/src/proto/` 的 message enum + `pigeon-host` 的处理分支 + `core/tests/proto` 往返 |
| 终端内核能力（换引擎 / 接别的内核） | `core/src/term/`（`Term` trait），C ABI、客户端、协议都不动 |
| 渲染、字体、输入法 | `Sources/PigeonApp/Terminal/`，不往下漏 |
| 一次性脚本 / 对账工具 | `tools/`，不放进 `Sources/` 或 `core/src/` |

### 可移植性硬规则（CI 跑 fmt / clippy / test / tokio+portable-pty 引用检查 / `--lib --no-default-features` 构建 / 体积上限 / 头文件 diff；仍缺 `unsafe` 与平台 crate 的静态检查，见 `docs/plan.md` §11）

- `core/` 里**不许**出现：平台专属 crate（`core-foundation`、`objc2`…）、`cfg(target_os = …)` 分支
 （PTY 与信号可以有一处，且必须两边都有实现）、直接操作窗口/字体/图形 API。
- `core/src/` 里**不许**出现 `unsafe`（唯一例外：`ffi.rs` 的 `extern "C"` 边界）。
- Swift 侧：`PigeonCore` 不许 `import AppKit`；`PigeonApp` 可以，但 `import CoreText` 只允许在
 `CoreTextRenderer.swift`。
- PTY 用 `portable-pty`（内部就是 `forkpty`/`openpty` 的封装）。因为 `core/src` 除 `ffi/` 外不许 `unsafe`，
  **不直接操作原始 fd**；也不用 `NSTask`/`Process` 之类 Apple 专属机制。
- 路径用 `FileManager`/POSIX；配置放 `~/.config/pigeon`（macOS 与 Linux 一致），不要用 `~/Library/...`。
- 信号（`SIGWINCH`/`SIGCHLD`）用 `signal()` 或 `SelfPipe`/`signalfd` 的思路，不依赖 macOS runloop 特性。
- 字体/渲染/输入法只在客户端层处理；`core/` 只搬字节与状态。
- `tokio` / `portable-pty` 是 **optional 依赖，只由 `host` feature 打开（默认开），且只允许在 `core/src/bin/` 里使用**。
  CI 会跑 `cargo build --lib --no-default-features`，并 grep 保证库本体不引用它们。

### 模块内的硬规则

- **一个文件一个主要类型**，文件名 = 类型名；单文件 > 400 行就拆。
- `core/src/term/` 是**唯一**实现 VT 语义的地方（包括 `alacritty_terminal` 的包装）；Swift 侧不得出现任何字节流解析。
- `core/src/status/` 是**唯一**实现 OSC 7501 解析与记录生命周期的地方。
- `core/src/proto/` 是**唯一**实现分帧与 CBOR 编解码的地方；客户端通过 C ABI 编解，不自己实现一份。
- `include/pigeon_core.h` 是**生成物**：改 ABI 就去改 `core/src/ffi/mod.rs`，然后跑 `cbindgen`，不要手改头文件。
- `SidebarModel` / `Grouping` / `KeyEncoder` 必须是**纯逻辑**（无 AppKit、无 IO），可直接单测。
- `pigeon-host` 里不能出现任何 UI 概念；`PigeonApp` 里不能出现 PTY 操作。
- 任何跨进程行为都通过 `core/src/proto/` 的显式消息表达；不要用共享全局状态。
- **终端语义只能在内核里实现**：VT 解析、快照/增量、布局计算、OSC 7501 记录都不许在客户端重复实现；
 客户端只做绘制与输入映射。

## 4. 代码风格

- Swift 6 / Rust 2021 edition，4 空格缩进；公开 API 写显式类型；`guard`/`if let` 早返回，避免深嵌套。
- Rust：提交前 `cargo fmt` + `cargo clippy -- -D warnings`；`unwrap()`/`expect()` 只允许在测试里，
  生产路径返回 `Result<T, PgError>`；不允许 `panic!` 越过 FFI 边界。
- **后台进程里禁止 `!` 强解包 / `try!`**；所有失败返回 `Result` 或 `throws` 的自定义 error enum。
- 并发：`core/` 不用可变全局；每个 pane 的状态由宿主串行驱（一个 task/线程一处所有权）；`App` 的 UI 状态只在主线程改。
- 命名：类型 UpperCamelCase，函数/变量 lowerCamelCase；协议名用名词（`Term`、`GridRenderer`）而不是 `-able` 广告。
- 注释只写“为什么”，不写“做了什么”；对外部协议的实现要在文件头注明依据（例如 OSC 7501 revision）。
- 语言约定：**代码与注释英文；`docs/` 中文；UI 文案英文**（与参考图一致，如 `Working · 1m`）。

## 5. 测试要求

- 新增解析规则（OSC 7501、按键编码、协议消息）→ **必须**带一个正例 + 一个畸形输入的反例（Rust 放 `core/tests/`，
  Swift 放 `Tests/`）。
- 修 bug → 先写能复现的测试，再修。
- VT 兼容性改动 → M1 起跑 `tools/vt-parity/run.sh`；现在靠 `core/tests` 的确定性用例（快照/宽字符/备用屏/真彩色）。
- 提交前必须全绿：`cargo test`（macOS + Ubuntu）+ `swift build && swift test`（macOS）。
- 测试不要依赖网络、不要依赖用户自己的 tmux/shell 配置；需要环境时自己在 `TMPDIR` 里造。

## 6. 常见任务的做法

**加一种状态显示**：`core/src/status/` 已有记录 → 只改 `SidebarModel` 的排序/图标映射 + `SidebarRowView`，不动协议。

**加一条通信消息**：`core/src/proto/` 的 message enum 加 case（带 version）→ 分帧/编解码自动覆盖 →
`pigeon-host` 加处理分支 → `core/tests/proto` 加编解码往返 → 重新生成 `include/pigeon_core.h` → 客户端订阅处处理。

**换 VT 引擎**：只替换 `core/src/term/` 的实现（`Term` trait），C ABI、客户端、协议都不动。

**加一个侧栏字段（如“最后活动时间”）**：`core/src/session/` 加字段 → `core/src/proto/` 带过去 →
重新生成头文件 → `SidebarModel` 用 → 测试。

## 7. 不变量（谁都不许破）

1. 关掉客户端 / 客户端崩溃，pane 里的进程必须继续跑（`pigeon-host` 持有 PTY）。
2. 重连后的屏幕必须与断开前一致（含备用屏、光标、滚动区）——用全量快照 + 增量，不靠重放历史字节。
3. OSC 7501 的丢弃规则严格按规范：超长、解不出 base64、解码后含控制字符 → **整条丢弃**，不能只丢一半。
4. `done` / `error` 记录在用户看过之前不能被静默清掉；`working` / `blocked` 在进程退出或 OSC 133A 提示符时必须清掉。
5. 不可信输入（程序写的 OSC 文本、链接、剪贴板）不得进入 UI 的富文本渲染路径；永远当纯文本处理。
6. 多客户端同看一个会话时，尺寸策略必须显式且可见（默认跟随最近活跃客户端，并在侧栏标出）。
7. `core/` 不得引入 Apple-only 依赖；`cargo test` 在 Ubuntu 红了就等于功能没做完。

## 8. 安全

- 一切来自 pane 的字节都是不可信的：`msg`/`title` 只当纯文本；拒绝控制字符；不解析成链接之外的标记。
- OSC 52（剪贴板）默认只写不读；读需要用户显式许可。
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

## 11. 设计文档在哪

- `docs/plan.md` —— 架构、选型、里程碑、验收、风险、已知缺口（**先读这个**）
- `docs/core.md` —— 内核边界、C ABI 说明、快照/增量、语言选型、测试策略
- `docs/protocol.md` —— 客户端 ⇄ 宿主的字段级契约（消息表、错误码、能力、尺寸策略）
- `include/pigeon_core.h` —— C ABI 的生成产物（唯一对外 ABI 面）
- `docs/remote.md` —— SSH 方案：远端宿主、三种对接、安装与安全
- `docs/gui.md` —— 前端路线：为何 AppKit/GTK、渲染与 IME 的归属
- `docs/ui.md` —— 侧栏与窗口的可见规格（含参考图，`docs/images/`）
- `docs/tmux-integration.md`、`docs/kitty-fork-status.md` —— 上一版（gmux）的实测记录，当反面教材读
