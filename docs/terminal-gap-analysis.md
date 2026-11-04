# 与代表性终端的差距评估（2026-11-04）

本评估基于当日仓库代码、文档和下列官方页面；不是同硬件性能排名，也不表示第三方能力已经逐项实测。
当前实现与缺口持续维护在 [plan.md §11](plan.md#11-当前能力与已知缺口)，本文保留本次分析依据与建议。
后续执行阶段、依赖和验收门槛见 [前沿终端优化计划](optimization-plan.md)；本文 §4 是评估时的建议，不作为执行进度表。

## 1. 判断

pigeon 的会话宿主、跨平台核心、原生客户端和 SSH 传输方向成立，macOS 已有日常终端的主要骨架。
目前与成熟终端的差距集中在文本保真、日常操作完整性、agent 状态可靠性、Linux 功能对齐和验收深度。
尚无证据支持“性能已超过/落后 Ghostty 多少”这类结论；已有 Metal 与 delta 不能替代端到端时延实测。

值得保留的产品方向是：**自动保持本机/远端会话，并把多主机上的 agent 请求准确汇总到原生侧栏**。
这是组合体验的机会；后台 mux、侧栏或 OSC 7501 单项都已有同行实现。

## 2. 应分别对照什么

| 参考终端 | 官方公开能力 | pigeon 应对照的部分 |
|---|---|---|
| [Ghostty 功能](https://ghostty.org/docs/features) | macOS/Linux 原生界面、Metal/OpenGL、连字、字素簇、Kitty 图像/键盘协议 | 文本/输入保真和平台体验。pigeon 已有 GPU 渲染，但字素与现代协议仍不完整 |
| [Kitty 概览](https://sw.kovidgoyal.net/kitty/overview/) | 可配置键位、远程控制、kittens、滚动历史操作、shell 集成、丰富的终端扩展 | 操作效率、扩展性和协议覆盖；其 [OSC 7501 支持](https://sw.kovidgoyal.net/kitty/misc-protocol/#reporting-the-status-of-a-program) 已可显示到标签栏、通过 `kitten @ ls` 读取、触发 watcher |
| [WezTerm mux](https://wezterm.org/multiplexing.html) | 本地/SSH mux domain、独立宿主、原生界面附加远端会话 | 持久化/重连的成熟度；pigeon 的按需安装小宿主和默认集成是可继续打磨的便利性 |
| [Warp Blocks](https://docs.warp.dev/terminal/blocks/) | 命令与输出成块，可独立复制、搜索、过滤、收藏和导航 | 长输出的可查找性、复制与命令导航。无需为了借鉴这些能力就改成聊天产品 |
| [cmux](https://www.cmux.dev/) / [通知](https://www.cmux.dev/docs/notifications) | 原生侧栏、agent 通知、CLI/socket 自动化、内置浏览器；官方还提供远端 tmux 对接与会话恢复 | 与 pigeon 最接近的 agent 工作流参照。pigeon 应先把状态准确性、跨主机跳转和可靠通知做完整 |

第三方的“会话恢复”需要区分恢复窗口/历史/重新启动 agent 与原进程继续存活；不能把它们直接等同于
pigeon 的宿主在客户端关闭后继续持有 PTY。

## 3. 已核实的关键缺口

### 文本数据在进入渲染器前已丢失

`core/src/term/alacritty.rs::convert_row` 只复制 `cell.c`，`term::Cell` 与协议 cell 都只能容纳一个码点；
macOS `CoreTextRenderer.rasterize` 和 Linux 图集也按单码点工作。

本次用现有 `vt-dump` 实测：

| 输入 | 导出的屏幕文本 | 说明 |
|---|---|---|
| `e` + U+0301 | `e` | 分解重音丢失 |
| 预组码点 `é` | `é` | 能显示预组字符不等于支持组合字符 |
| `👩` + U+200D + `💻` | `👩💻` | ZWJ 丢失，无法完整重建原字素 |

这不是换字体能解决的。应先保留完整字素及布局所需信息，经快照、增量、协议、两种客户端接口送达渲染器，
再做跨码点整形。编程连字可以后续增加，先保证文字不丢。

### 高频操作仍有明显断点

- 没有终端搜索；macOS 选择和 Select All 只覆盖当前屏幕。
- `TerminalSelection.text` 在每个网格行之间插入换行，且快照不保留软换行标志；长命令复制可能带入额外换行。
- OSC 8 的链接信息被归零；OSC 52 读写事件都被忽略。本地菜单复制粘贴已实现，远端程序主动复制到本地剪贴板未实现。
- 两端 KeyEncoder 仍为 legacy。Shift+Enter、按键事件类型、现代 TUI 的扩展键位需要完整的键盘协议协商与编码。
- 字体仅启动时读配置，缺完整自定义键位、主题配置、热重载和命令面板。

依据：`Terminal/PaneView+Selection.swift`、`TerminalSelection.swift`、`KeyEncoder.swift`、`FontConfig.swift`、
`core/src/term/alacritty.rs`、`clients/linux/src/input.rs`。

### agent 收件箱需要先保证准确

macOS 已有 `app`/`msg`/`kind`、状态计时、未读、needs-you、跨主机侧栏和跳转。
但 `SessionReducer.apply` 把最新收到的 pane 状态写进会话行，没有按所有 pane 的状态聚合；
一个 pane 的 idle 可以覆盖另一个 pane 的 blocked。记录来源 pane 与 ack 目标也需要一起核对。

此外，当前启动时只注入 `PI_PROGRAM_STATUS=1`；其它 agent 能在终端里运行，并不意味着会自动上报状态。
缺少通用上报工具、接入说明、支持矩阵和桌面通知。可以提供由用户显式启用的通用上报方式，继续保持
“状态只来自 OSC 7501、不猜屏幕、不擅改用户配置”的边界。

与 cmux 相比，用户会先感到接入与通知的缺失；与 Kitty 相比，OSC 7501 本身已经不是独有能力。

### Linux 还处于首片阶段

已有 GPU 网格、本地宿主、分屏、zoom、焦点和 resize。但侧栏/会话选择、SSH 管理、回看、选择/剪贴板、IME、
鼠标/焦点上报、括号粘贴、delta 和自动重连尚未接齐。当前 CI 与发布流程也未覆盖 GTK 客户端。
应把 Linux 的日常操作闭环当成明确的交付范围；不能从“核心能在 Linux 编译”推导“产品已跨平台对齐”。

### 性能与可靠性缺的是可比较的证据

已有 Metal 渲染线程、脏行、输入回显优先 flush、出站合并/上限、断线续传与回看切片优化。
建议继续测：输入到屏幕的 p50/p95/p99、16 pane 输出下的交互、远端 50/100/200ms RTT、10 万行回看、
空闲 CPU/唤醒/功耗。10 万行是压力测试目标，宿主当前固定 1 万行，需要测试配置后才能完成该场景。

现有 8 条 VT golden 只比较文本，不覆盖完整样式、字素、输入、GPU 像素、IME 或可访问性。
本次还发现宿主继承启动环境的 `TERM`：`TERM=linux` 时 vim 备用屏用例超时，显式 xterm 后通过，说明环境隔离仍需补强。

## 4. 建议顺序（待排期）

| 优先级 | 工作 | 验收方向 |
|---|---|---|
| P0 正确性 | pane 级状态聚合与 ack、组合字符保真、软换行复制、TERM 环境隔离 | 两个 pane 的状态互不覆盖；文本跨快照/增量/复制不丢；宿主测试不受外部 TERM 影响 |
| P1 日常使用 | 搜索、跨历史选择、OSC 8、OSC 52 写、Kitty keyboard protocol | 构建日志能查找、长命令能原样复制、远端编辑器复制有效、常用 TUI 键位正确 |
| P1 核心体验 | 通用 OSC 7501 上报说明/工具、桌面通知、准确定位待处理 pane | 新用户能自行接入，blocked 可发现、可定位、不会被其它 pane 清掉 |
| P1 Linux | 剪贴板/IME/回看 → 侧栏与会话选择 → SSH/恢复 → delta；加入 CI/发布 | 至少完成与 macOS 对应的日常工作流程，并明确尚未对齐的功能 |
| P2 交付与性能 | 键位/主题配置、签名公证、端到端性能和兼容性基线 | 可安装、可配置、更新可信；性能变化有固定环境和可重复数字 |
| P3 扩展 | 图片协议、编程连字、命令块、自动化接口 | 按真实用户工作负载决定；浏览器、内置 AI、插件系统不必一起做 |

现有分层可以继续用。是否更换 VT 引擎应由具体协议覆盖成本和实测决定；即使换引擎，
当前每格单码点的快照/协议仍需扩展，无法自动获得所有现代能力。

## 5. 本次验证边界

- `tools/vt-parity/run.sh`：8/8，通过已有 golden，没有重录。
- `TERM=xterm-256color cargo test --manifest-path core/Cargo.toml --offline --quiet`：194 passed、2 ignored（含文档测试）。默认 `TERM=linux` 下有上述一个失败。
- Unicode 探针使用现有 `vt-dump`，未修改产品代码或测试。
- 本次未运行 macOS GUI、Swift 或 GTK GUI 验收，未做第三方终端的同硬件性能测试。
