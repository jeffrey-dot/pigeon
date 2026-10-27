# AGENTS.md

给编码 agent 的工作说明。人看的部分在 `README.md`，产品/架构在设计文档里（见 §12）。
**当前状态：只有文档，还没有代码。**本文件里的目录结构是**约定**，M0 落地时按它建。

## 1. 项目是什么

pigeon 是一个**跨平台**终端（macOS 首发，Linux 跟进），自带隐形后台会话进程与一等公民侧栏：
一个窗口里左边是按项目分组的会话列表（带 agent 状态、未读、`needs you` 计数），
右边是该会话的终端本体。任务活过窗口关闭（`pigeon-host` 持有 PTY 与屏幕状态）。

硬性边界：

- **跨平台分层**：`PigeonShared` 与 `PigeonHost` 必须能在 macOS 和 Linux 上编译运行；只有 `PigeonApp` 是平台专属。
- **不依赖 tmux**，也不 shell out 到 tmux。
- **不 fork 其它终端**；VT 内核用 SwiftTerm（MIT），藏在 `TerminalBackend` 协议后面。
- **不修改用户的 shell/tmux 配置**，不安装任何东西到用户目录之外。
- 状态只来自 **OSC 7501**，不做屏幕/标题启发式识别。

## 2. 环境与命令

macOS + Xcode 命令行工具即可（`swift --version` ≥ 6）。

```sh
swift build                     # 同一个二进制的窗口+后台两个角色（两平台都要能过）
swift test                      # 单元测试（CI 双平台：macOS + Ubuntu）
swift run pigeon-host           # 会话宿主（本机自动拉起；远端按需推送；手动跑用于调试）
swift run pigeon                # 客户端（M0 起可用）
tools/vt-parity/run.sh          # VT 对账：同一段字节流喂我们与参考实现，逐屏比较
```

这些命令在对应模块落地前会失败——**先建模块，再加功能**，不要为了跑通而把逻辑塞进 `main.swift`。

## 3. 模块划分（强制）

依赖方向只有一条：`App → Shared ← Daemon`。`App` 与 `Daemon` **互相不可见**。

可移植性标记：

- `core/`（Rust）—— **portable**：不许依赖任何平台 API；`unsafe` 只允许出现在 `lib.rs` 的 FFI 边界。
- `Sources/PigeonShared/`、`Sources/PigeonHost/` —— **portable**：只许 Foundation + POSIX，必须能在 Linux 编译并通过测试。
- `Sources/PigeonApp/` —— **macOS only**：AppKit/SwiftTerm 视图都在这里；不要往下泄漏。
- 将来的 Linux 客户端是**另一个 target**（例如 `PigeonAppLinux`），只依赖 `PigeonShared` 的协议与模型，
  不 import `PigeonApp`，也不 import `PigeonHost`。

```
pigeon/
├── AGENTS.md
├── Package.swift                 # targets: PigeonApp / PigeonHost / PigeonCore / PigeonShared / PigeonTests
├── docs/                         # plan.md, ui.md, 历史资料
├── tools/vt-parity/              # 对账脚本与 golden 文件（不含产品代码）
├── include/pigeon_core.h         # 对外的 C 契约（字段带 version/size）
├── core/                         # Rust 内核（crate，平台无关）
│   ├── Term/                     # VT：SwiftTerm 的 Terminal 包装 + 快照/脏行/滚动
│   ├── Layout/                   # 布局树：嵌套 vsplit/hsplit、比例、最小尺寸
│   ├── Status/                   # OSC 7501 记录与生命周期
│   ├── Session/                  # session/window/pane 模型
│   └── CAPI.swift                # @_cdecl 导出上方 C 契约（唯一对外的 ABI 面）
├── Sources/
│   ├── PigeonCore/               # Swift 侧的 C 头封装（薄，只做类型映射，不实现语义）
│   ├── PigeonApp/                # 客户端：只做 UI 与输入
│   │   ├── PigeonApp.swift       # @main，装配依赖
│   │   ├── Window/               # 窗口、标题栏、红绿灯、窗口级快捷键
│   │   ├── Sidebar/
│   │   │   ├── SidebarModel.swift      # 分组/排序/未读/计数（纯逻辑，可单测）
│   │   │   ├── SidebarViewController.swift
│   │   │   ├── SidebarRowView.swift    # 一行：名称 + 相对时间 + 状态图标
│   │   │   └── Grouping.swift          # 项目分组规则（git 根 / cwd 末级目录）
│   │   ├── Terminal/
│   │   │   ├── PaneView.swift          # 终端视图容器（焦点、IME、鼠标）
│   │   │   ├── TerminalBackend.swift   # 协议：feed / snapshot / resize / respond
│   │   │   ├── SwiftTermBackend.swift  # 唯一 import SwiftTerm 的文件
│   │   │   └── KeyEncoder.swift        # 按键 → 字节（kitty protocol / legacy）
│   │   └── Layout/
│   │       ├── LayoutTree.swift        # 嵌套 vsplit/hsplit、比例、最小尺寸（纯逻辑）
│   │       ├── SplitView.swift         # 把 LayoutTree 画成 AppKit 视图
│   │       └── Focus.swift             # 焦点与选择
│   ├── PigeonShared/             # 窗口与后台进程共用；**不许 import AppKit**
│   │   ├── Protocol/
│   │   │   ├── Message.swift           # 所有消息的 enum（Codable，带 version）
│   │   │   ├── Codec.swift             # 编解码
│   │   │   └── Framing.swift           # 分帧（长度前缀），不许用换行分隔
│   │   ├── Model/                      # Session / Pane / Project / Status 纯数据
│   │   ├── Status/
│   │   │   ├── OSC7501.swift           # 单条报告的解析与校验（对照规范逐条实现）
│   │   │   └── StatusStore.swift       # 记录生命周期：替换/clear/上限/淘汰
│   │   └── Util/                       # 小工具（相对时间、路径缩写、git 根）
│   └── PigeonHost/               # 会话宿主 pigeon-host（本机+远端同一二进制，3–8MB 硬指标）；**不许 import AppKit**
│       ├── main.swift            # 参数解析与启动
│       ├── Server.swift          # unix socket、多客户端、权限（0700）
│       ├── SessionManager.swift  # 会话/窗口/pane 的创建与生命周期
│       ├── Pane.swift            # 一个 pane：PTY + backend + 状态记录
│       ├── PTY.swift             # forkpty / 读写 / SIGWINCH / 退出码
│       └── ProtocolHandler.swift # Message → SessionManager 的映射
└── Tests/
    ├── StatusTests/              # OSC 7501：规范里的例子 + 畸形/超长/分包/clear
    ├── ProtocolTests/            # 编解码往返、版本兼容、半包
    ├── LayoutTests/              # 布局树：比例、最小尺寸、嵌套、删除后归并
    └── VTParityTests/            # 与参考实现逐屏对账（golden 文件）
```

### 新增代码放哪里（判定表）

| 你要加的东西 | 放这里 |
|---|---|
| 一种新的 OSC/控制序列的处理 | `Shared/Status/`（若是状态）或 `Shared/Protocol/`（若是通信）；VT 渲染相关的放 `TerminalBackend` 实现 |
| 窗口与后台进程都要知道的数据 | `Shared/Model/` |
| 只在 UI 里用的派生状态（排序、分组、文案） | `App/Sidebar/SidebarModel.swift`（保持纯函数，方便单测） |
| 新的通信消息 | `Shared/Protocol/Message.swift` + `ProtocolHandler.swift` 的分支 + 测试 |
| 终端内核能力（换 SwiftTerm / 接别的内核） | 只新增一个 `TerminalBackend` 实现，不动 UI |
| 一次性脚本 / 对账工具 | `tools/`，不放进 `Sources/` |

### 可移植性硬规则（CI 会拦）

- `PigeonShared` / `PigeonHost` 里**不许**出现：`import AppKit`、`import Cocoa`、`NSTask`、`NSWindow`、
  `CoreGraphics`、`CoreText`、`#if os(macOS)`（平台分支只能出现在 `PigeonApp` 与 `TerminalBackend` 的
  实现里）。
- PTY 一律 `openpty`/`forkpty` + `FileHandle`，不要用 `NSTask`/`Process` 之外的 Apple 专属机制。
- 路径用 `FileManager`/POSIX；配置放 `~/.config/pigeon`（macOS 与 Linux 一致），不要用 `~/Library/...`。
- 信号（`SIGWINCH`/`SIGCHLD`）用 `DispatchSourceSignal` 或 `signal()`，不依赖 macOS runloop 特性。
- 字体/渲染/输入法只在客户端层处理；后台进程只搬字节与状态。

### 模块内的硬规则

- **一个文件一个主要类型**，文件名 = 类型名；单文件 > 400 行就拆。
- `SwiftTermBackend.swift` 是**唯一**允许 `import SwiftTerm` 的地方；其余代码只用 `TerminalBackend` 协议。
- `SidebarModel` / `LayoutTree` / `StatusStore` / `OSC7501` 必须是**纯逻辑**（无 AppKit、无 IO），可直接单测。
- `PigeonHost` 里不能出现 `NSWindow` / `NSView` / 任何 UI 概念；客户端里不能出现 PTY 操作。
- 任何跨模块行为都通过 `Shared/Protocol/Message.swift` 的显式消息表达；不要用共享全局状态。
- **终端语义只能在内核里实现**：VT 解析、快照/增量、布局计算、OSC 7501 记录都不许在客户端重复实现；
  客户端只做绘制与输入映射。

## 4. 代码风格

- Swift 6，4 空格缩进；公开 API 写显式类型；`guard` 早返回，避免深嵌套。
- **后台进程里禁止 `!` 强解包 / `try!`**；所有失败返回 `Result` 或 `throws` 的自定义 error enum。
- 并发：`PigeonHost` 用 actor 串行化会话状态；`App` 的 UI 状态只在主线程改。
- 命名：类型 UpperCamelCase，函数/变量 lowerCamelCase；协议名用名词（`TerminalBackend`）而不是 `-able` 广告。
- 注释只写“为什么”，不写“做了什么”；对外部协议的实现要在文件头注明依据（例如 OSC 7501 revision）。
- 语言约定：**代码与注释英文；`docs/` 中文；UI 文案英文**（与参考图一致，如 `Working · 1m`）。

## 5. 测试要求

- 新增解析规则（OSC 7501、按键编码、协议消息）→ **必须**带一个正例 + 一个畸形输入的反例。
- 修 bug → 先写能复现的测试，再修。
- VT 兼容性改动 → 跑 `tools/vt-parity/run.sh`；golden 文件变更要在 PR 里说明原因。
- 提交前必须全绿：`swift build && swift test`（macOS 与 Ubuntu 两个 job）。
- 测试不要依赖网络、不要依赖用户自己的 tmux/shell 配置；需要环境时自己在 `TMPDIR` 里造。

## 6. 常见任务的做法

**加一种状态显示**：`StatusStore` 已有记录 → 只改 `SidebarModel` 的排序/图标映射 + `SidebarRowView`，不动协议。

**加一条通信消息**：`Message.swift` 加 case（带 version）→ `Codec` 自动覆盖 → `ProtocolHandler` 加分支 →
`Tests/ProtocolTests` 加编解码往返 → 客户端订阅处处理。

**换 VT 引擎**：只替换 `core/src/term/` 的实现（`Term` trait），C ABI、客户端、协议都不动。

**加一个侧栏字段（如“最后活动时间”）**：`Shared/Model` 加字段 → `PigeonHost` 填 → 协议带过去 → `SidebarModel` 用 → 测试。

## 7. 不变量（谁都不许破）

1. 关掉客户端 / 客户端崩溃，pane 里的进程必须继续跑（`pigeon-host` 持有 PTY）。
2. 重连后的屏幕必须与断开前一致（含备用屏、光标、滚动区）——用全量快照 + 增量，不靠重放历史字节。
3. OSC 7501 的丢弃规则严格按规范：超长、解不出 base64、解码后含控制字符 → **整条丢弃**，不能只丢一半。
4. `done` / `error` 记录在用户看过之前不能被静默清掉；`working` / `blocked` 在进程退出或 OSC 133A 提示符时必须清掉。
5. 不可信输入（程序写的 OSC 文本、链接、剪贴板）不得进入 UI 的富文本渲染路径；永远当纯文本处理。
6. 多客户端同看一个会话时，尺寸策略必须显式且可见（默认跟随最近活跃客户端，并在侧栏标出）。
7. `PigeonShared` / `PigeonHost` 不得引入 Apple-only 依赖；Linux CI 红了就等于功能没做完。

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
- 合并前：`swift build && swift test` 全绿；涉及 VT 的改动附对账结果。

## 11. 设计文档在哪

- `docs/plan.md` —— 架构、选型、里程碑、验收、风险（**先读这个**）
- `docs/ui.md` —— 侧栏与窗口的可见规格（含参考图，`docs/images/`）
- `docs/tmux-integration.md`、`docs/kitty-fork-status.md` —— 上一版（gmux）的实测记录，当反面教材读
