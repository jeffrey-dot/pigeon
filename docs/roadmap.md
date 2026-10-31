# 优化方案（2026-10-31）

目标：一个前沿的终端，能连远端服务器并像 tmux 一样保住远端会话；侧栏列出所有会话和里面每个 AI agent 的状态；
美观、流畅、好用。本方案来自三份通读审查（Swift 客户端、Rust 内核/宿主、UI/渲染），结论按“用户先感受到什么”排序。

## 结论

架构方向是对的，**不推倒重写**：Rust 内核（`term/`、`status/`、`proto/`、`layout/`、`backlog`）和 Metal 渲染器都保留。
要重写的是两块胶水：客户端的 `PigeonApp.swift`（1575 行，所有 bug 都在这里）和宿主的 `pigeon-host.rs`（1551 行）。
真正拖后腿的不是架构，而是：**核心卖点（agent 状态）实际上看不到**，以及**终端最基本的能力缺失**（复制粘贴、输入法、鼠标）。

## 阶段与顺序

### P0 正确性（先做，都小）

| 问题 | 现状 | 改法 |
|---|---|---|
| `blocked` 在正在看的会话上一闪就没 | 客户端对选中会话的每条 status 都 ack，宿主随即回 `idle` | 只 ack `done`/`error`；`working`/`blocked` 由进程退出或 133A 清 |
| status 的 `kind`/`msg`/`app` 没用上 | 宿主不发 `app`/`id`，客户端只读 `state`/`seq` | 协议 `status` 加可选 `app`、`id`、`since_ms`；客户端存下来给侧栏和头部用 |
| 分屏里一个 shell 退出，整个会话被关 | `pane_exited` → `closeSession` 给所有 pane 发 `close_pane` | 只关退出的那个 pane；会话没 pane 了才算关闭 |
| 后台主机的事件改写窗口 | `activeHost` 被临时换掉时调 `refreshSidebar`/`updateHeader` | 刷新侧栏一律在恢复 `activeHost` 之后做；ack 只对可见主机的选中会话 |
| `markRead` / 未读键 / 改名 | 写错键、`activeHost` 没恢复、`renamed` 没人读 | 统一用 `(host, session)` 键；改名接到侧栏标题 |
| 写 socket 可能 SIGPIPE 崩溃、EINTR 丢半帧 | 只在退化模式里忽略 SIGPIPE | 进程启动即 `SIG_IGN`；写循环处理 EINTR/EAGAIN |
| `hello_error` 不提示 | `close()` 不触发 `onClose` | 关闭后走 `hostLost` 显示原因 |
| 第二个宿主会删掉活着的 socket | 启动先 `unlink` | 先尝试连接，连得上就退出；bind 前设 umask 0077 |
| Kill/Resize 用 `try_send` 会丢 | 满了就丢 | 改为等待发送 |

### P1 终端基本功（“好用”的底线）

1. **选择 + 复制粘贴**：拖选 / 双击选词 / 三击选行，⌘C/⌘V/⌘A，Edit 菜单；粘贴走 bracketed paste。
2. **输入法**：`PaneView` 实现 `NSTextInputClient`（marked text、候选窗定位），中文/日文/emoji 面板可用；⌥ 作 Meta 可配置。
3. **终端模式上 ABI**：快照/增量带 `modes`（DECCKM、鼠标上报、bracketed paste、焦点上报）；
   客户端据此编码方向键、发 SGR 鼠标事件。
4. **光标**：按 `shape` 画块/竖线/下划线，失焦时空心，可闪烁。
5. **DIM 属性**、**方框字符程序绘制**（`─│┌┐└┘` 连成线）。

### P2 agent 面板（核心卖点）

- 侧栏行变成两行：上行 = agent 名 + 状态 + 计时；下行 = 当前任务或 blocked 原因（`msg`），只在 working/blocked 时出现。
- working 用转动的环，blocked 一次柔和脉冲 + 琥珀色；计时从状态开始的时刻算（宿主给 `since_ms`）。
- 右栏头部显示 `Working · 3m · Refactoring the renderer` / `Needs you · permission · Run cargo test?`。
- 宿主侧：不 attach 也能收到所有会话的 status（`sessions` 里带每会话的最新状态），status-only 的客户端不参与尺寸投票。

### P3 流畅

- 宿主 `scroll` 只切出一屏，不再每步转换 1 万行历史（实测 14.5ms/步）。
- 输入后立即 flush 一帧（打字回显不再等 16ms 节拍）。
- 客户端主线程缓存字体度量和色板（现在每帧查 `NSFont(name:)` 和 16 次 `bestMatch`）。
- 写 socket 移出主线程（串行队列 + 有界发件箱），慢 ssh 不再卡界面。

### P4 结构重写

- 客户端：每台主机一个 `HostConnection`（自带 rows/seq/焦点/重连状态机），`AppStore` 管选择与持久化状态，
  侧栏 id 用 `SessionRef{host, session}`，取代 `activeHost` 来回切换和 `host * 100_000 + session` 编码。
  消息处理写成纯 reducer，可单测。
- 宿主：`pigeon-host.rs` 拆成 cli / transport / client / dispatch / pane_actor；`Session` 与尺寸策略下沉到 `core/src/session/`；
  每客户端出站队列（事件只编码一次，慢客户端只留每 pane 最新 delta，Layout/Status/Exited 不丢）。

### P5 视觉

- 深色：卡片 `#0E0F11`、侧栏 `#16171A`，正文 `#E6E6E6`；唯一强调色琥珀 `#F0B35C` 只给 needs you。
- 浅色：黄/青调到对白底至少 3:1。
- 字体与字号可配置（`~/.config/pigeon/config.toml`），默认 SF Mono 13 / 行高 1.25；侧栏数字用等宽数字。
- 动效：选中/状态变化 120ms ease-out。

### P6 远端持久化增强

- 远端宿主状态目录用 `~/.pigeon/state`（不依赖会在登出时被删的 `$XDG_RUNTIME_DIR`）。
- 宿主定期把每 pane 的屏幕快照 + 元数据落盘；宿主重启后会话以“进程已结束”的只读屏幕恢复。
- 心跳与空闲超时，半开的 ssh 连接不再永远占着尺寸投票。

## 不做

推倒重写内核或换 UI 框架；屏幕启发式识别 agent（仍只认 OSC 7501）；Windows。
