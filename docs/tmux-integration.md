# gmux 与 tmux 的交互方式（以及哪些 tmux 配置会影响它）

写于 2026-10-27，对应 `kitty/` 里这套 fork。所有结论都来自本机实测（tmux 3.8 / macOS 27）。

## 0. 一句话

gmux **不改 tmux、不读 tmux 的配置文件、不装 tmux 插件、不要求 DCS passthrough**；
它只做两件事：**在 pty 里跑一个 tmux 客户端**，以及**用命令行查询/操作用户已有的 tmux server**。
所有状态都来自 tmux 自己，gmux 不解析 `~/.tmux.conf`，也不依赖任何 tmux 扩展。

## 1. 两条交互路线

### 路线 B（默认）：把 tmux 当普通客户端

```text
kitty 窗口
  └─ pty  ──► tmux -L <socket> attach -t <session>[:<window>]
                     │
                     └─ tmux 自己渲染：多 pane、边框、状态栏（gmux 会按 session 关掉状态栏）
```

- gmux 只提供窗口；按键、鼠标、resize 全部由 tmux 客户端自己处理（gmux 不转译按键）。
- OSC 7501 走不到 gmux（tmux 丢弃不认识的 OSC），所以另起一个**隐形 control client** 专门收状态（见 §4）。
- 每个 kitty tab = 一次 `tmux attach -t session:window`（当前版本：默认只开 1 个 tab + 左侧栏切换窗口）。

### 路线 A（`GMUX_TMUX_CONTROL=1`）：control mode，自己画

```text
helper 进程（fork 出来的）
  ├─ pty ──► tmux -L <socket> -C attach -t <session>      ← 控制通道，收 %output 等事件
  └─ pty ──► kitty（把 pane 字节当“子进程输出”喂进去）
```

- 只画**一个** pane（pane 字节是按它自己的屏幕坐标寻址的，见 `docs/kitty-fork-status.md`）。
- 按键走 `send-keys -H`，尺寸走 `refresh-client -C`，切换走 `select-window/select-pane`。

## 2. 用到的 tmux 命令（全部经 `-L <socket>`，默认 socket 名就是 `default`）

| 目的 | 命令 | 谁在用 |
|---|---|---|
| 发现 session | `list-sessions -F '#{session_name}\t#{session_activity}'` | 启动时选“最近用过的 session” |
| 检查/建 session | `has-session -t <s>`、`new-session -d -s <s> [-e PI_PROGRAM_STATUS=1]` | 启动 |
| 接管 | `attach -t <s>`（B）/ `-C attach -t <s>`（A） | 主通道 |
| 客户端尺寸 | `refresh-client -C <cols>x<rows>` | A（B 由 tmux 客户端自己报） |
| 查活动 pane | `display-message -p -t <s> '#{pane_id}'` | A |
| 列 pane/window | `list-panes -s -t <s> -F '…'`、`list-windows -t <s> -F '…'` | A、侧栏、watcher |
| 键盘 | `send-keys -H <hex …>` | A |
| 切换 | `select-window -t @N`、`select-pane -t %N` | A、侧栏点击 |
| 重画 | `capture-pane -p -e -J -t %N` | A（切 pane / 新视图） |
| 读前缀键 | `show-options -gv prefix` | A |
| 读/关状态栏 | `show-options -t <s> status`、`set -t <s> status off\|on` | 启动/退出 |
| 标注状态 | `rename-window -t @N '<mark> <name>'` | watcher |
| 退出 | `detach-client` | A |

## 3. 数据流

**状态（OSC 7501）**：pane 里的程序（Pi 等）往自己的 pty 写 `ESC ] 7501 ; … ST` →
tmux 收到但不认识，**在路线 B 里直接丢弃**；唯一能拿到它的是 control mode：

```text
pane ──OSC 7501──► tmux ──%output(含 OSC)──► watcher（隐形 control client）
                                              └─ 解析出状态，写 /tmp/gmux-status-<socket>-<session>.json
侧栏（kitty split 里的 TUI）每 0.4s 读那个 JSON，画树 + 标记
```

**窗口/布局**：全部是 tmux 的真相 —— gmux 只读 `list-windows` / `list-panes`，
需要切换时发 `select-window` / `select-pane`，从不在本地维护一份“自己的布局”。

## 4. tmux 配置会不会影响 gmux

会。逐项说清楚（✅ 免疫 / ⚠️ 有影响 / ❗ 有副作用）：

| tmux 设置 | 影响 | 现状 |
|---|---|---|
| `prefix` | ⚠️ 路线 A 里前缀键由 gmux 自己实现，只认 `C-x` 形式；`M-a`、`F1` 这类会退回 `C-b` | `show-options -gv prefix` 读一次；非 `C-x` 未支持 |
| `base-index` / `pane-base-index` | ✅ 免疫：gmux 只用 `@window-id` / `%pane-id` 和 tmux 报的 `#{window_index}` 显示，从不假设从 0 或 1 开始 | 已实测（用户配置是 `base-index 1`） |
| `status`（状态栏） | ❗ gmux 启动时把它**按 session** 关掉、退出时打开（`GMUX_TMUX_STATUS=keep` 可保留）。注意：恢复时写的是 `on`，如果用户原本就是 `off`，会被打开 | 实测 3.8 支持 session 级 `status`；更老版本未验证 |
| `window-size`（默认 `latest`） | ⚠️ 一个 window 的尺寸跟“最后活动的客户端”。gmux 的 client 和用户终端里的 client 会互相牵制；同 session 多客户端时会跳尺寸 | 未改这个选项；建议 gmux 用独立 session |
| `automatic-rename` | ⚠️ watcher 的 `rename-window '<mark> <name>'` 会被 tmux 的自动改名覆盖（命令一变就重命名） | 每次状态变化重写；不保证长期显示 |
| `status-*` / `set-titles` | ⚠️ 状态栏被关掉后这些设置对 gmux 无意义；用户自己的终端仍按他们的配置显示 | 不改 |
| `default-terminal` / `terminal-features` / `terminal-overrides`（真彩色等） | ⚠️ 路线 B 里 tmux 按**客户端 TERM** 决定颜色/真彩色。gmux 的 attach 子进程继承启动它的环境变量（`TERM` 可能是 `xterm-256color`，不一定等于用户的 `xterm-kitty`） | 未强制设 `TERM`；如果颜色不对，需要在启动前 `TERM=xterm-kitty` |
| `extended-keys` / `csi-u`（tmux ≥3.5） | ⚠️ 影响能否区分 Shift+Enter 等组合键：路线 B 由 tmux 按客户端 TERM 决定；路线 A 下 gmux 转发原始字节，tmux 的 `extended-keys` 设置起决定作用 | 未改动，靠用户配置 |
| `escape-time` | ⚠️ 路线 B 直接吃用户的 `escape-time`（影响 Esc 延迟）；路线 A 不经过它 | 不改 |
| `mouse` | ✅ 路线 B 完全交给 tmux；路线 A 里 gmux 发的是原始按键字节，鼠标由 tmux 的 `mouse` 决定是否使用 | 不改 |
| `set-clipboard`（OSC 52） | ✅ 路线 B 由 tmux 处理；路线 A 由 kitty 处理 | 不改 |
| `allow-passthrough` | ✅ 与 gmux 无关：gmux 走 control mode，不需要 DCS 包装 | — |
| `history-limit` / scrollback | ⚠️ 路线 B 用 tmux 自己的 scrollback；路线 A **没有** scrollback（只用 `capture-pane` 取可见屏） | 已知缺口 |
| `destroy-unattached on` | ❗ **危险**：关闭 gmux 最后一个客户端时 tmux 会销毁 session，任务会丢 | gmux 不检查这个选项；文档提醒 |
| `remain-on-exit` | ⚠️ 影响 pane 进程退出后 pane 是否还在（进而影响“agent 已退出”的判断） | watcher 用“前台命令是否回到 shell”判断 |
| `update-environment` | ⚠️ 决定 `PI_PROGRAM_STATUS=1` 能否进入新 pane。gmux 建的 session 用 `new-session -e` 直接注入，不受它影响；已有 session 里的 pane 需要用户自己设 | 文档已说明 |
| hooks（`set-hook`） | ✅ gmux 不装 hook、不依赖 hook；与用户的 hook 无冲突 | — |
| `prefix` 之外的 keybindings | ✅ 路线 B 全部照用户配置生效；路线 A 下 tmux 收不到自己的 key table（control mode 固有行为），前缀键由 gmux 复刻了一小部分 | 见 `docs/kitty-fork-status.md` |

### 版本相关

- `refresh-client -C`、`%output`、`%begin/%end`：control mode 基础，老版本也有；**实测 3.8**。
- `%extended-output`：≥3.4（gmux 两种都解析）。
- `new-session -e`：≥3.2（gmux 用它注入环境变量）。
- session 级 `status`：**实测 3.8 可用**，更老版本不确定。
- 目前**没有**启动时的版本检查（`tmux -V` 只在我手工测试时装过），这是一个待补项。

## 5. gmux 会写进用户 tmux 的东西（全部可撤销）

| 写操作 | 何时 | 如何撤销 |
|---|---|---|
| `set -t <s> status off` | 启动（路线 B） | 退出时 `atexit` 恢复 `on`；`GMUX_TMUX_STATUS=keep` 可完全跳过 |
| `rename-window -t @N …` | 每次 OSC 7501 状态变化 | 状态 `clear` 时改回原名；tmux 自动改名也会覆盖 |
| `new-session -d -s <name>` | 完全没有 session 时 | 用户 `tmux kill-session` |
| `select-window` / `select-pane` | 点侧栏、路线 A 切 pane | 这是 session 的“当前窗口”，**对同一 session 的其它客户端也生效**（用户终端会跟着跳） |
| 键盘/鼠标 | 用户在 gmux 里操作 | 与在终端里操作等价 |

gmux **不会**：写 `~/.tmux.conf`、`source-file`、改 `window-size`、装插件、重启 server、kill 别人的 pane。

## 6. 读哪些环境变量

| 变量 | 作用 |
|---|---|
| `GMUX_TMUX_SESSION` | 要接管的 session（不设时自动选最近用过的，没有就建 `gmux`） |
| `GMUX_TMUX_SOCKET` | tmux socket 名，默认 `default` |
| `GMUX_TMUX_CONTROL=1` | 切到 control mode（路线 A） |
| `GMUX_TMUX_STATUS=keep` | 保留 tmux 自己的状态栏 |
| `GMUX_TMUX_AUTO=0` | 关闭“启动即连 tmux” |
| `PI_PROGRAM_STATUS=1` | gmux 新建的 session 会注入给 pane，让 agent 上报 OSC 7501 |

## 7. 代码位置

| 文件 | 职责 |
|---|---|
| `kitty/tmux_control.py` | control mode 客户端：pty、协议解析、查询重试、OSC 7501 解析、`parse_layout()` |
| `kitty/tmux_child.py` | 把一个 pty 包成 kitty 的“子进程”：路线 B 直接 exec `tmux attach`，路线 A 用 helper 进程桥接 control mode |
| `kitty/tmux_status_watch.py` | 隐形 control client：收 OSC 7501 → window 名 + 桌面通知 + 状态 JSON |
| `kitty/tmux_sidebar_tui.py` | 左侧栏（当前是字符 TUI，画 session→window→pane 树） |
| `kitty/tabs.py`、`kitty/boss.py` | 启动接线：选 session、起侧栏、起 watcher、关状态栏 |

## 8. 为什么 tmux 会丢掉 OSC 7501，自己写一个 multiplexer 是否免疫

### 结论

是 tmux 的**默认行为**，不是配置问题：

- tmux 对每个 pane 都是一个**完整的终端模拟器**（它自己维护屏幕、光标、备用屏），所以它必须
  逐条处理控制序列。它认识一批 OSC（0/1/2 标题、4/10/11/12 颜色、7 cwd、8 超链接、9/777/99
  通知、52 剪贴板、104/110/111/112、133 shell 集成、1337 iTerm2 等），**不认识的就地丢弃**。
- OSC 7501 不在它的列表里（截至 tmux 3.8），所以：普通客户端（`tmux attach`）永远看不到它；
  **只有 control-mode 客户端**能看到，因为 `%output` 给的是 pane 的**原始字节流**，不是渲染结果。
  这也是 gmux 在路线 B 里还要挂一个隐形 control client 的原因。
- 唯一让普通客户端看到它的办法是程序自己用 **DCS passthrough** 包一层
  （`ESC P tmux; ESC <OSC 7501 …> ESC \`）并要求 `allow-passthrough on`。Pi 不做这件事，
  所以对 Pi 无效。

### 自己做一个 multiplexer

**是免疫的，而且这是设计自由度问题，不是运气**：

- 如果你写的是**模拟器型**（像 tmux 一样自己画 pane）：控制序列怎么处理由你定。正确默认是
  “认识的处理，**不认识的原样转发给外层终端**”（OSC 7501 规范也要求终端忽略不认识的 OSC，
  所以转发是安全的）。这样以后任何新协议都不用改你的代码。
- 如果你写的是**代理型**（自己不画，每个 pane 一个真实终端窗口/split，你只负责 PTY 与布局）：
  你根本不需要 VT 解析，OSC 天然原样通过。kitty 多开窗口 + 布局管理就属于这一类，成本比写
  一个终端模拟器低一个数量级。

### 最小 tmux 需要的东西（模拟器型）

1. 每 pane 一个 PTY + fork/exec；
2. 一个 VT 解析器（光标、SGR、备用屏、宽字符、鼠标上报、括号粘贴）—— **这块是大头**；
3. 布局（窗口/分屏 + 尺寸协商）；
4. 输入路由（按键 → 活动 pane；前缀键由你自己定义）；
5. 控制序列策略（见上）：已知的处理/存储，未知的转发。

如果走**代理型**，第 2 项可以直接没有，代价是“一个 pane 一个系统级窗口/split”，
不能像 tmux 那样在一个终端里嵌任意矩形布局。

### 对 gmux 的含义

- 路线 A（control mode）里 OSC 7501 在 tmux 边界就已经被我们取走，**不依赖** tmux 是否认得它；
- 路线 B 只能靠隐形 control client 旁路拿状态；
- 将来如果 gmux 自己渲染 pane（或干脆自己当 multiplexer），这条旁路就可以去掉。
