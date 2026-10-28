# pigeon 开发计划（原生终端 + 隐形后台会话进程）

> 2026-10-27 改版。上一版是「fork Kitty + 用 tmux 承载 pane」；实测后放弃：
> Kitty 的界面是 C/OpenGL 画的固定控件，做不出原生侧栏；tmux 又会把它不认识的 OSC 7501 丢掉，
> 状态还得靠旁路 client 抢。结论：**自己做终端 + 自己做最小 multiplexer**，参考成熟项目而不是 fork。
>
> 历史与证据：`~/dev/gmux/docs/tmux-integration.md`（tmux 交互与配置影响）、`~/dev/gmux/docs/kitty-fork-status.md`（fork 阶段实测）。

## 1. 产品范围

### 首发（v0.1 Beta）必须有
- 桌面应用，**macOS 首发，Linux 是明确的第二个目标**。窗口内是真正的终端：vim/htop/less/Pi TUI 都能跑。
  跨平台的做法见 §3.2：后台会话进程与协议全平台共用，客户端按平台实现。
- **左侧栏是一等公民**：按 `项目/会话 → 窗口 → pane` 分组的树，行尾是状态图标 + 相对时间，
  组头有 `1 needs you` 计数，可折叠；点击切换、可跳到下一个等待中的位置。
- 分屏与多窗口：任意矩形布局（vsplit/hsplit 嵌套），可拖动分隔线，可放大单个 pane。
- **会话持久化**：关掉窗口/应用崩溃后任务继续跑；重开自动接回（等于 tmux 的 detach/attach）。
- 状态：直接消费 **OSC 7501**（程序自己上报 idle/working/blocked/done/error），
  不再有中间层能把它吃掉；blocked 桌面通知、未读标记、kill -9 后回落“未监控”。
- 本机 + SSH 主机：连远端时把**同一个后台会话进程**放到远端跑（见 §3.3）。

### 首发不做
- Windows、多人协作/共享会话、云同步、内置 SSH 密码管理。
- 屏幕/标题启发式识别（不做 spinner 正则——OSC 7501 就是为了替代它）。
- 对某个 agent 的专用适配（Pi/Claude Code 只要上报 OSC 7501 就一样用）。

## 2. 目标架构

```text
┌──────────────── pigeon 客户端（AppKit + Swift）───────────────────┐
│ 侧栏 SidebarModel ← StatusStore（OSC 7501）                     │
│   │                                                             │
│ 分屏视图 ← LayoutTree（我们自己的矩形树，不是 tmux layout 字符串）│
│   │                                                             │
│ PaneView（终端视图，每个 pane 一个）：渲染 + 键盘/鼠标/IME        │
└───────────────────────────┬─────────────────────────────────────┘
                            │ unix socket（本机）/ ssh 转发（远端）
┌───────────────────────────┴─────────────────────────────────────┐
│ pigeon-host（无窗口，也可推到远端）：每个 pane 一个 PTY            │
│   ├─ pigeon-core（Rust，C ABI）：完整 VT 状态机 + 布局 + 状态记录   │
│   ├─ LayoutTree：会话/窗口/pane 的几何与焦点                      │
│   ├─ StatusStore：解析 pane 字节流里的 OSC 7501                   │
│   └─ 协议：attach 全量重绘 / 增量屏幕 diff / 键盘 / 鼠标 / resize │
└─────────────────────────────────────────────────────────────────┘
```

要点：
1. **VT 状态机在后台进程里**，这样应用崩溃后重连还能把屏幕原样画回来（等于我们在 tmux 上验证过的
   `capture-pane` 复原）；也避免了“客户端决定真相”的漂移。
2. **协议保持极简**：客户端 attach 时先拉全量快照（含光标、备用屏、滚动区），之后收增量；
   键盘/鼠标/尺寸走同一条连接。参考 tmux control mode 的 `%output` 思路，但我们自己定。
3. **OSC 7501 由我们解析**：我们就是终端，不再有 tmux 那种“不认识的 OSC 直接丢”的中间层。
   顺带把 OSC 9;4（进度）、OSC 52（剪贴板）、OSC 133（shell 集成）一起支持。
4. 布局是我们自己的树（嵌套 vsplit/hsplit + 比例），**不用 tmux 的 layout 字符串**；
   解析函数可以留着做导入工具（一次性迁移 tmux 布局）。

## 3. 技术选型

### 3.1 三条路线

| 路线 | 内核 | 客户端 | 优点 | 代价 |
|---|---|---|---|---|
| **A（已选）Rust 内核 + C ABI** | `alacritty_terminal` / `vte` 起步，包在自家 `Term` trait 后 | macOS：AppKit / Linux：GTK4（详见 `docs/gui.md`） | 内核只写一次、所有平台与语言共用；C ABI 稳定；内存安全 + 性能上限高；已装 Rust（minimal 工具链） | 多一门语言 + 多一套构建 + FFI 胶水；M0 稍慢 |
| B Swift 内核 + C ABI 导出（备选） | SwiftTerm 的 `Terminal`（headless，支持 Linux） | 同 A | 零 FFI、零新工具链，M0 最快 | 性能上限低；被第三方复用时生态弱 |
| C libghostty-vt 内核 | Ghostty 的 VT 核心 | 自建 | 内核现成 | 依赖上游演进；仍需自建布局/会话层 |

**结论：走 A —— 内核用 Rust（`alacritty_terminal` / `vte`，包在自家 `Term` trait 后），经 `cbindgen` 导出 C ABI，客户端按平台薄薄一层（前端路线见 `docs/gui.md`）。** 详细设计见 **`docs/core.md`**
（边界、C 头、快照/增量、线程与所有权、测试策略）。
**为什么是 Rust**：VT 状态机是最容易攒长尾 bug 的地方，`alacritty_terminal`/`vte` 是已经被大量终端验证过的实现，
自己重写不划算；C ABI 又是天然的跨语言边界，GTK 客户端与将来的第三方消费者都从同一条缝进来。
代价是「两门语言 + 两套构建 + 一层 FFI」，换来的是 M5 之前不必再赌一次内核重写。

**什么时候改回 Swift（备选 B）**（触发任一即重新评估，届时换的是 `Term` 实现，UI/协议/C ABI 都不动）：
1. Rust 在 macOS 交叉编译 / 静态链接（musl）上出现绕不过去的阻塞，撑不住 `docs/remote.md` 的 8MB 与单文件硬指标；
2. FFI 的调试与构建成本超过收益（例如 cbindgen 覆盖不了某个必要能力）；
3. 确定要做 Windows 客户端，且 Swift 的交叉编译链反而更省事。

### 3.2 跨平台策略（macOS 首发，Linux 跟进）

分层的关键：**缝在协议上，而不是在 UI 上**。

| 层 | 平台 | 说明 |
|---|---|---|
| `core/`（VT / 布局 / OSC 7501 / 协议 / 会话） | 全平台 | Rust；只许 std + POSIX 绑定；无 UI、无字体、无图形 API；`unsafe` 只在 `ffi.rs` |
| `pigeon-host`（PTY + 会话宿主；本机与远端同一个二进制） | 全平台 | `core/` 里的 bin；用 `portable-pty`（forkpty/openpty 的安全封装）；静态单文件 3–8MB（见 `docs/remote.md`） |
| `include/pigeon_core.h` | 全平台 | 由 `cbindgen` 从 `core/src/ffi/mod.rs` 生成，宿主与所有客户端共用的唯一 ABI 面 |
| 客户端 | 按平台 | macOS：**AppKit**（`NSOutlineView` 侧栏 + 自绘网格，M0 首发）；Linux：**GTK4**（M5）。为什么不用 winit/Qt/Electron 见 `docs/gui.md` |

- **协议是跨语言契约**：长度前缀 + 版本号，字段语义写进 `docs/protocol.md`（M0 随快照复原一起产出）。Linux 客户端可以用
  Swift + GTK，也可以换语言重写——内核与宿主一行不改。
- 如果将来更看重“一套 UI 代码跑两个平台”，那就要接受非原生侧栏外观（winit / Flutter / Tauri 路线），
  那是 `docs/gui.md` 的方案 C/D，首发不选。
- **CI 从 M0 起就双平台**：macOS + Ubuntu 各跑一遍 `cargo test`（内核与宿主是跨平台的）；
  `swift build && swift test` 只在 macOS（`PigeonApp` 是平台专属）。

### 3.3 后台会话进程是什么（对你隐形，不用配置）

**一个没有窗口、关不掉窗口也依然活着的后台进程。**

普通终端（kitty、Terminal.app）里，**窗口进程自己持有 shell 的 PTY**：窗口一关，PTY 关闭，
shell 和它跑的东西一起收到 SIGHUP 死掉。所以“关窗口 = 任务没了”。

tmux 的做法就是把这件事拆成两半：

```text
tmux server（后台，持有 PTY + 屏幕内容）   ←  你连上去的每个终端只是"显示器"
      ↑
tmux attach（前台，一个普通程序）
```

你用过的那条体验——**关掉终端，tmux 里的任务还在；再 `tmux attach` 回来，屏幕原样**——
这就是 tmux 的模型。pigeon 把它做成**同一个程序的两个角色**，对你完全隐形。

| | 谁持有 shell 的 PTY | 关掉窗口后 | 重开能接回 |
|---|---|---|---|
| 普通终端（kitty/iTerm） | 窗口进程 | 进程被杀（SIGHUP） | ❌ |
| tmux | `tmux server` | 继续跑 | ✅ |
| **pigeon** | **`pigeon-host`** | 继续跑 | ✅（连回去要一份屏幕快照即可重画） |

具体来说，后台进程负责：

1. 为每个 pane 开 PTY、跑你的 shell/agent（窗口进程完全不碰 PTY）；
2. 维护每个 pane 的**终端状态**（屏幕、光标、备用屏、滚动历史）——所以断线重连能原样画回来；
3. 记住会话/窗口/分屏的布局；
4. 解析程序上报的 OSC 7501，得到“在干活/在等你/完成了”；
5. 在 `/tmp/pigeon-<uid>.sock` 上听窗口：`给我 pane 3 的快照`、`这些是键盘字节`、`窗口变成 120x40`。

**启动方式（和 tmux 一模一样，没有任何服务/配置）：**

```text
打开 pigeon.app
   ├─ 连 /tmp/pigeon-501.sock
   │    ├─ 连上 → 复用（列出已有会话）
   │    └─ 连不上 → fork 一个 pigeon-host（setsid 脱离父进程）→ 再连
   └─ 拿快照，画窗口

关窗口 → 窗口进程退出，后台继续跑
再打开 → 连上、要快照、原样恢复
```

两个产物：`Pigeon.app`（窗口）与 `pigeon-host`（无窗口的会话宿主，3–8 MB，本机自动拉起、远端按需推送）。

**本机是不是装了两个应用？** 不是。`pigeon-host` 打包在 `Pigeon.app/Contents/Helpers/` 里，
随 app 一起签名/公证/升级；Dock 里只有一个图标、/Applications 里只有一个 app。
运行时 `ps` 会看到两个进程（`Pigeon` + `pigeon-host`）——和 Chrome 的 helper、VSCode 的后端进程一样。
关窗口 = 窗口进程退出、`pigeon-host` 继续；`⌘Q` = 连 `pigeon-host` 一起退出（会提示还有几个会话在跑）。
想只跑一个进程也可以（本地直接由 app 进程持有 PTY），但那样本地就失去“关窗口任务继续”，所以默认不这么做。
不需要 systemctl/launchd，不需要 root，不开网络端口，`ps` 里只看到一个 `pigeon-host`。

### 3.4 远端（SSH：同一后台进程装到服务器上）

远端跑**同一个** `pigeon-host`（3–8 MB 的小宿主，自动推送安装），本机窗口通过系统 `ssh` 连过去，接管服务器上的终端。
细节见 **`docs/remote.md`**，三条要点：

1. **三种对接**（自动探测并明示）：`ssh host pigeon-host --proxy`（stdio 桥接，推荐）→ unix socket 直转 →
   退化模式（普通 ssh，仍能显示 OSC 7501 状态，但没有远端持久化）。
2. **安装**：客户端首次连接时自动推送并安装（先打印命令与哈希再执行，`--no-remote-install` 可关）；
   推对应架构的静态二进制 + sha256 校验；不静默覆盖已有版本、不改远端 shell 配置。
3. **安全**：只经 ssh，不开端口；远端只监听 0700 的 unix socket；`--proxy` 只转发协议帧；
   认证/密钥/known_hosts 全交给系统 `ssh`。

## 4. OSC 7501 作为状态通道

- 我们是终端 ⇒ 直接在 PTY 字节流里解析 `ESC ] 7501 ; … ST`（`kitty/tmux_control.py` 里的
  `StatusStore` 逻辑可以直接搬：按 `id` 建记录、4096 字节上限、base64 解码、控制字符拒绝、
  `clear` 删子树）。
- 状态归属：`pane` 级记录 → 侧栏把 window/项目级徽标按 `blocked > error > done > working > idle`
  聚合；`blocked` 优先并带 `kind`（permission/question/auth）与 `msg`。
- 生命周期（规范把这件事交给终端）：pane 进程退出或出现 OSC 133A 提示符 → 丢弃 `working/blocked`；
  `done/error` 保留到用户看过该 pane。**我们是终端，所以能准确知道进程何时退出**——比 tmux 旁路更可靠。
- 让程序开口：新建 pane 时注入 `PI_PROGRAM_STATUS=1`（Pi 用），其它程序各自有开关；
  未上报的显示“未监控”，不做启发式。

## 5. 值得读的参考项目（按用途）

| 项目 | 语言 | 借鉴什么 |
|---|---|---|
| **SwiftTerm** | Swift | **只作对账与规格来源，当内核的是 Rust**：读 `Terminal.swift`（VT 状态机）、`HeadlessTerminal.swift`（无 UI 跑终端）、`AppleTerminalView.swift`（AppKit 视图与输入法/鼠标）、`ProgramStatus.swift`（OSC 7501 的参考实现），用来喂 `tools/vt-parity` 比对 |
| **Ghostty / libghostty-vt** | Zig | 干净的“VT 核心 ↔ UI”分层；`libghostty-vt` 的嵌入接口是我们路线 C（Ghostty 内核）的蓝本 |
| **kitty** | C + Python | `screen.c` / `parser.c` 的 VT 细节，`child-monitor.c` 的 PTY 事件循环与背压（`pause-after`）；它也是“性能怎么做到”的参照 |
| **Alacritty** | Rust | 极简内核 + `vte` crate（独立可复用的 VT 解析器）；配置热重载的做法 |
| **WezTerm** | Rust | 它自带 mux（`Mux`/`Tab`/`Pane` 结构）与“多路复用 + 原生 UI”的整体设计 |
| **Zellij** | Rust | 会话/布局模型（KDL 布局）、插件化、客户端-服务端分离 |
| **tmux** | C | 会话/窗口/pane 语义、`input.c` 的控制序列处理、**control mode 协议**（我们已实测）；布局字符串解析留着做导入 |
| **dvtm / mtm / abduco / dtach** | C | 极简 multiplexer 与“只做会话保持”的最小实现，适合对照我们后台进程的边界 |
| **libvterm / pyte** | C / Python | 参考实现，用来**对账**我们内核的输出（见 §7） |
| **Harp / cmux / Herdr / Agent Deck / TUIOS / Rex** | 各类 | **侧栏与状态呈现**的产品参照：分组、`needs you` 计数、相对时间、未读点、点击跳转 |

## 6. 最小 multiplexer 的能力清单（按优先级）

1. **PTY + 生命周期**：每个 pane 一个 pty，fork/exec；退出、信号、`SIGHUP` 语义。
2. **VT 状态机**：光标、SGR（含真彩色）、备用屏（1049/1047/47）、滚动区（DECSTBM）、宽字符/组合字符、
   自动换行、制表位、字符集、鼠标上报、括号粘贴、焦点事件、OSC 0/2/4/8/52/133/7501、
   DA/DSR 应答（我们是终端，必须答）。
3. **布局**：嵌套 vsplit/hsplit、比例与最小尺寸、拖动分隔线、放大（zoom）。
4. **输入**：按键编码（含 kitty keyboard protocol 与 legacy 模式）、死键/IME、粘贴、鼠标。
5. **会话**：attach/detach、全量快照重绘、尺寸协商（多客户端时取哪个尺寸——**这点要显式定义**，
   tmux 的 `window-size latest` 就是坑）。
6. **滚动**：滚动缓冲（历史）、搜索。
7. 之后：多客户端同看（tmate 式共享）、布局导入 tmux、插件。

## 7. 里程碑

| 里程碑 | 交付 | 验收 |
|---|---|---|
| **M0 单 pane + 后台会话进程** | Rust 内核（C ABI）+ AppKit 客户端 + `pigeon-host` + 快照复原 | vim/htop/less/Pi TUI 正常；中文/宽字符/真彩色；resize；无重复应答；**关窗口后进程继续跑、重开屏幕原样** |
| **M1 分屏** | 自己的 LayoutTree + 多 pane | 左右/上下/三格/四格嵌套；拖动分隔线；缩放；焦点切换；关窗口后所有 pane 继续跑 |
| **M2 多客户端与健壮性** | 协议版本协商、多窗口同看一个会话、后台进程崩溃处理 | 两个窗口看同一会话尺寸正确；`kill -9` 后台进程有明确提示并可重开；协议版本不匹配拒绝连接并说明 |
| **M3 侧栏** | 树 + 状态 + 未读 + 通知 + 跳转 | 分组与计数正确；`blocked` 优先；后台 pane 状态可见；kill -9 后 ≤5s 回落；点击/键盘都能跳 |
| **M4 SSH** | 宿主自动推送到远端 + 连接管理 | 断网 30s 恢复；远端任务不受影响；认证失败/远端没跑起来有明确提示 |
| **M5 Linux 客户端** | 复用同一宿主与协议，Linux 原生客户端（GTK）/ 或换语言的客户端 | Ubuntu 上功能对齐 macOS：会话、分屏、侧栏、OSC 7501 状态；`cargo test` 在 Ubuntu 全绿（Swift 侧只跑 macOS） |
| **M6 Beta** | 性能、IME、剪贴板、双平台打包签名 | 三档机器上 16 pane × 大输出不卡；macOS 分发包可安装；Linux 有 AppImage/tar |

M0–M1 可以直接复用现有代码：内核的对账语料（来自 tmux 阶段录制的字节流）、SwiftTerm 原型作为**对照实现对账**、
`StatusStore`（OSC 7501 解析）、`~/dev/gmux/app/compat.py` 的对账思路。

## 8. 测试与验收

| 维度 | 方法 |
|---|---|
| VT 兼容性 | `vttest` 全套 + 与参考实现（tmux 的 `capture-pane`、`libvterm`、pyte）逐屏对账：同一段字节流喂两边，比较屏幕与属性（这套方法在 `~/dev/gmux/app/compat.py` 里已经跑通过） |
| 真实程序 | vim/neovim、htop、less、fzf、Pi TUI、cargo/terraform 进度 |
| 输入 | 中文输入法（拼音候选窗位置）、Shift+Enter（kitty keyboard protocol）、Option 作为 Meta、粘贴多行 |
| 会话 | 关窗口 / 杀客户端 / 杀后台进程 / 断网；重连后屏幕与滚动一致性 |
| 性能 | 16 pane、每 pane 每秒 1MB 输出、滚动 10 万行；记录帧时间与内存 |
| 状态 | OSC 7501 全状态 + 畸形/超长/分包/`clear`/256 条上限；killed agent 的回落 |

## 9. 主要风险

| 风险 | 影响 | 对策 |
|---|---|---|
| VT 兼容性是长尾（kitty 花了十年） | 程序显示错乱 | 不自研内核（路线 A/B）；对账测试常态化 |
| 后台进程协议设计不当 | 重连丢屏/错位 | 先做“attach 全量快照 + 增量”两段式，快照包含备用屏与滚动区；用 tmux control mode 踩过的坑做输入 |
| 多客户端尺寸冲突 | pane 尺寸跳变 | 显式策略：默认跟随“最近活跃”的客户端，并在侧栏显示当前尺寸来源；允许锁定尺寸 |
| 原生 IME 与终端光标位置 | 候选窗错位 | 参考 SwiftTerm 的 IME 处理；M1 就测拼音 |
| 性能（无 GPU 也要能看） | 大输出卡顿 | M1 用 CoreText/图层缓存；M5 前评估 Metal |
| Linux 客户端等于第二份 UI | 双倍 UI 工作 | 协议先行（`docs/protocol.md` 字段级）；`core/` 从第一天就 Linux 可编译，CI 双平台；交互规格 `docs/ui.md` 对齐两边行为 |
| Swift 生态在 Linux 上的坑（GUI 绑定、字体、输入法） | Linux 版体验差 | M5 前只保证后台进程/协议在 Linux 可跑；客户端选型留到 M5（GTK 绑定 vs 换语言） |

## 9.5 许可证

**GPL-3.0-or-later**（`LICENSE`）。影响两条设计决策：
1. 依赖只能选 GPLv3 兼容的：SwiftTerm（MIT）、`alacritty_terminal`/`vte`（MIT/Apache-2.0）、libghostty-vt（MIT）、
   GTK（LGPL，动态链接）都可用；不要静态链接 GPL 不兼容的第三方代码。
2. 参考项目只借“思路与规格”，逐行复制需要同许可证并标注来源；`docs/tmux-integration.md` 里的 tmux 行为记录属于规格事实，不受影响。

## 10. 现有资产（可直接搬）

| 资产 | 位置 | 用途 |
|---|---|---|
| OSC 7501 解析与记录规则 | `~/dev/gmux/kitty/kitty/tmux_control.py`（`StatusStore`）、`kitty/tmux_child.py` | 搬进后台进程；规则与规范逐条对应 |
| 状态旁路/回落逻辑（kill -9、shell 回落、通知） | `~/dev/gmux/kitty/kitty/tmux_status_watch.py` | 我们是终端后可简化，但“未监控回落”的经验保留 |
| 终端界面/输入/渲染原型 | `~/dev/gmux/native/`（Swift + SwiftTerm + 侧栏雏形） | 只作**对账参考**与 M0 临时验证，不作为产品内核 |
| 兼容性对账脚本 | `~/dev/gmux/app/compat.py` | 改成“我们的内核 vs 参考实现” |
| tmux 交互与配置影响的实测记录 | `~/dev/gmux/docs/tmux-integration.md` | 设计后台进程与尺寸策略时的反面教材 |
| fork 阶段的结论 | `~/dev/gmux/docs/kitty-fork-status.md` | 为什么不 fork kitty、为什么不做字符侧栏 |

## 11. 已知缺口

M3 落地后仍在的洞。

**已关闭**：

| 旧缺口 | 关闭方式 |
|---|---|
| M0 文档层：`core/`、`Package.swift`、生成的头文件、`cbindgen.toml`、`Tests/`、C ABI 编解码 | 全部落地，见 `docs/m0-checklist.md` |
| 布局只有单 pane | `core/src/layout/` 的布局树 + `pg_layout_*` 全实现；宿主每 pane 独立 PTY/actor；客户端按 `PgRect` 分屏 |
| `zoom_pane` / `unzoom` 未实现 | 协议按规范回 `unsupported` 且不断开（解码与宿主各有用例）；UI 见下表 |
| 多客户端尺寸策略未实现 | 单客户端下由 pane rect 反推会话 box；多客户端的 `size_state` 仍留给 M2 |
| 程序的颜色查询（OSC 4/10/11） | 客户端通过 `appearance` 把当前色板交给宿主，宿主用 `Event::ColorRequest` 的 formatter 回 `rgb:rrrr/gggg/bbbb`；TUI（pi 等）因此能自己选亮/深主题 | 见 `docs/protocol.md` §3 与 `core/tests/host.rs` 的 `appearance_reaches_existing_panes_and_a_malformed_one_keeps_the_previous` |
| 侧栏（M3） | 已落地：`SidebarModel`/`Grouping`（git 根）+ 侧栏、OSC 7501 `status` 广播 + `ack_status`、OSC 7/标题、未读/needs-you、⌘J/⌘T/⌘⇧W、右键菜单、**行标题=应用名 / 分组可改名 / 不折叠**、排序持久化；视觉按 harp 源码对齐：`docs/ui.md` §7 是从 harp（`/tmp/harp`，AGPL/MIT，只取数值不取代码）提取的色值/尺寸/字号规格表；行高 30、行圆角 8、组头 11.5/橙色 `N need you`、`SIDEBAR #e8e8eb/#232326`、透明标题栏 + 内嵌红绿灯、右栏是一张大卡片（10pt 圆角、0.5pt 描边、投影、三面 8pt 外缩）、40pt 右栏头部，全部是动态色（浅/深各一套）；截图见 `docs/m3-checklist.md` §2.1 的 light/dark 两套 |

**仍在**：

| 缺口 | 现状 | 何时关闭 |
|---|---|---|
| `zoom_pane` / `unzoom` 的 UI | 宿主与协议都回 `unsupported`，客户端未绑定快捷键 | M2 |
| 悬停 `×` 关闭 / 透明标题栏 | `docs/ui.md` §2.1/§2.3 写了，未实现：关闭走右键菜单或 `⌘⇧W`；红绿灯仍在原生标题栏 | 改窗口 chrome（`titlebarAppearsTransparent`）时 |
| 未上报 session 的相对时间 | 没有 OSC 7501 记录的 session 行尾留空，不拿创建时间冒充“最后活动时间” | 宿主能上报 pane 活动时间时（M4） |
| `tools/vt-parity/run.sh` 与 golden 语料未建 | 靠 `core/tests/` 的确定性用例兜底 | M2 接真实程序对账时 |
| OSC 8 超链接被丢弃 | `PgCell.hyperlink_id` 恒为 0，`cell[4]` 不上线 | 需要按链接跳转时 |
| DECSTBM 滚动区恒等于整屏 | `alacritty_terminal` 不暴露 `scroll_region` | 需要客户端画滚动条时 |
| Swift 客户端不协商 `delta` | 只收 `snapshot`（协议 §5 允许） | 做性能时开启增量 |
| IME 候选窗未实现 | 走 AppKit 默认 `keyDown` | M6 |
| `zstd` 能力只定义了帧内 tag | 阈值、何时启用未定 | M4 远端实测后 |
| CI 缺 `unsafe` / 平台 crate 的静态检查 | 现有：fmt / clippy / test / tokio+portable-pty 引用检查 / `--lib --no-default-features` / 体积 / 头文件 diff / swift | 加 `unsafe`/`cfg(target_os)` 检查时 |
| PTY 走 `portable-pty` 而不是原始 fd | 为了让 `core/src`（除 `ffi/`）零 `unsafe` | 需要原始 fd 做特殊 ioctl 时 |
| `swift test` 依赖完整 Xcode | `xcode-select` 指向 CLT 时需 `DEVELOPER_DIR` | 换 swift-testing 或写进 README |

**平台说明**：`pigeon-host` 在 macOS 与 Linux 用同一份代码；CI 的 `rust` 作业在 ubuntu 与 macos 各跑一遍
`cargo test`（含 spawn 真实 PTY 的宿主用例）。首跑记录：
M0 <https://github.com/jeffrey-dot/pigeon/actions/runs/40633073432>（`4adc398`，64 passed）、
M1 <https://github.com/jeffrey-dot/pigeon/actions/runs/40635575813>（`6637f5c`，86 passed）。
