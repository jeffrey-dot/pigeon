# Kitty fork 进度与设计结论（2026-10-27）

fork 基线：Kitty `v0.49.2`（`kitty/`，独立 git，未提交）。

## 已完成

| 里程碑 | 内容 | 证据 |
|---|---|---|
| M0 身份隔离 | `appname='gmux'`、`Gmux.app`、bundle id `net.kovidgoyal.gmux`、独立 `~/Library/Caches/gmux` | 与原版 kitty 并存运行，原版配置不受影响 |
| M1 control mode 客户端 | `kitty/tmux_control.py`：`%output` / `%extended-output` / `%begin-%end`（带重试）/ `%layout-change` / `%exit`；`refresh-client -C WxH`；`send-keys -H`；按 pane 的 OSC 7501 记录；`parse_layout()` | `python3.12 -m kitty.tmux_control --selftest` 8/8 通过 |
| M2 pane 渲染 + 键盘 | `kitty/tmux_child.py`：fork 一个 helper 进程持有 pty，把 pane 字节喂给 kitty、把按键转成 `send-keys -H` | 实时输出、中文、vim、resize 实测；DA1 只收到一次应答 |
| M3 window↔tab | 启动时为每个 tmux window 建一个 kitty tab。路线 B 下每个 tab 就是另一个 `tmux attach -t session:index`；路线 A 下每个 tab 一个 control client 并 `select-window` | 实测双 window：2 个 kitty tab（1/2），tab 2 显示 `WINDOW-ONE`，tmux 状态栏 `[demo] 0:bash- 1:bash*`，窗口尺寸稳定 146x39 |
| 切换 pane | 活动 pane 变化时 helper 用 `capture-pane` 重画 | `tmux select-pane` 后画面跟着换 |

改动：`kitty/constants.py`、`setup.py`、`kitty/tabs.py`、`kitty/boss.py`、`kitty/options/definition.py`（`close_on_child_death` 默认改 yes）+ 新增 `kitty/tmux_control.py`、`kitty/tmux_child.py`。

## 三条挡路的实测结论

1. **kitty 主循环一启动，Python 线程就冻结**（1 Hz 心跳线程只跑一次）。任何阻塞 I/O 都不能放在 kitty 进程的线程里 → 桥接必须放进 fork 出来的 helper 进程（已这样做）。
2. **kitty 的 Python 侧看不到 pty 字节**（C 层 `child-monitor` 直接喂给 `Screen`），而且没有通用的“监听 fd → 回调 Python”接口。所以 helper 无法通知 GUI：layout 变化、状态变化都不能在 Python 侧被动响应 ⇒ 动态 tab/split 同步、状态侧栏都需要 C 侧支持（或在主循环里轮询，但那会卡帧）。
3. **pane 字节是 pane 自己的屏幕坐标**，不是窗口坐标。一个 kitty 窗口只能画一个 pane ⇒ 真多 pane 必须“每 pane 一条 pty + 一个 kitty split”，并且只能有**一个** control client 负责 `refresh-client -C`（多个 client 会互相改 tmux window 尺寸，`window-size latest`）。

## 剩余工作（按结论 3 落地）

**M3 剩余：pane↔split**
- helper 建 N 条 pty（每 pane 一条），通过 `socketpair` + `SCM_RIGHTS` 把 master 端交给父进程（`socket.send_fds`，Python 3.9+ 可用）。
- 父进程按 tmux layout 递归建 kitty split 树（`vsplits`/`hsplits` 权重 = layout 的宽高比例），并把每个 pane 的 pty 挂到对应 window。
- 尺寸只由“主 client”发 `refresh-client -C`（tab 总面积），其余 client 不发。
- 动态同步（外部 tmux 改动 1s 内跟随）需要结论 2 的通道：最小改动是给 `child-monitor.c` 加一个“任意 fd 可回调 Python”的入口（约 50 行 C），或把 control-mode 解析整体下沉到 C。

**M4 侧栏**
- 依赖结论 2 的同一条通道。备选（零 C 改动）：侧栏信息写进 tmux pane 选项（`set -p @gmux_*`），由 tmux 自己的 status-line 显示——但这会改用户的 tmux 配置，且与“不修改 tmux”的原则冲突，需用户点头。

**另一条可选路线（零 C 改动，换取侧栏）**
- pty 里跑**普通** `tmux attach`（不是 `-C`）：tmux 自己把多 pane + 边框 + 状态栏合成一帧，kitty 直接渲染 → 多 pane 立刻可用，layout 完全交给 tmux。
- 代价：拿不到 per-pane 流，OSC 7501 会被 tmux 丢掉 → 状态侧栏暂时没有。

## 本轮新增的两个实验

- **状态→tmux window 名**：helper 收到 OSC 7501 后 `rename-window "● permission <name>"`，实测
  `bash → ● permission tmux → ◐ tmux → tmux`（clear 会还原）。注意：这只在**别人的终端**的
  tmux status bar 里看得见；gmux 自己的窗口只画 pane，看不到状态栏，所以对 gmux 本身等于不可见。
- **路线 B（pty 里跑普通 `tmux attach`，`GMUX_TMUX_RAW=1`）**：已修好 —— 先给 pty 一个初始
  `TIOCSWINSZ`，tmux 立刻绘制。实测：左右两个 pane + tmux 边框 + tmux 状态栏全部正确（截图）。
- **路线 B 现在是默认**（`GMUX_TMUX_CONTROL=1` 才走 control mode）。实测默认启动即为
  `146x40 / panes=2`，多 pane 与状态栏都正常。
- **状态接回（`kitty/tmux_status_watch.py`）**：路线 B 下 tmux 会丢掉 OSC 7501，所以再挂一个
  **隐形 control client** 专门收状态，把结果写进 window 名：实测 `bash → ● permission bash → bash`。
  这条在路线 B 里看得见（tmux 状态栏），也顺带让用户自己的终端看到同样的标记。
  还带：① `blocked` 时 `osascript display notification` 桌面通知；② 每秒问一次 tmux，
  agent 进程没了（pane 前台命令回到 shell）就把记录丢掉 —— 实测 `kill -9` 后约 3 秒
  从 `● permission bash` 回落成 `bash`。

## 侧栏（2026-10-27 晚）

- kitty 的**竖向标签栏**（`tab_bar_edge=left`，gmux 默认注入）就是侧栏：每个 tmux window 一个 tab，
  标题是 `<徽标> <kind> <index>:<name> — <msg>`。
- `kitty/tmux_sidebar.py`：主循环定时器（`fast_data_types.add_timer`，0.5s）比对 tmux window 列表
  与 tab，**动态增删 tab**（实测外部 `tmux new-window` 后 2 秒内多出 tab）。
- tmux 自己的**底部状态栏只对当前 session 关掉**（`set -t <session> status off`，退出时 atexit 恢复；
  `GMUX_TMUX_STATUS=keep` 可保留）——桌面端不该有两套状态显示。
- 状态来源：`tmux_status_watch` 写 `/tmp/gmux-status-<socket>-<session>.json`，侧栏定时器读它上标题。
- watcher 生命周期绑到 app pid（`GMUX_WATCH_PARENT`），app 退出后不会残留。

## 默认路线说明（2026-10-27 决定）

默认走 B：**tmux 自己渲染多 pane + 状态栏，gmux 只提供窗口**，OSC 7501 由旁路 watcher 接回。
优点：今天就能日常用、布局永远是 tmux 真相、gmux 崩溃不影响任务。
代价：没有 kitty 自己的 pane↔split、没有左侧栏（这两项要等 C 侧 fd→Python 回调）。

## 零 C 的“动态 tab”备选（未实现，供决策）

tmux 的 `set-hook -g window-linked/window-unlinked` 可以跑 `run-shell`，脚本里用 kitty 的
remote control（`kitten @ launch --type=tab 'tmux -L sock attach -t session:N'`）就能在
tmux 新建/关闭 window 时动态加/删 kitty tab —— 不需要改 C。代价：hook 是 server 级的，
会影响同一台 tmux server 上的其它客户端，需要按 session 限定或接受这个副作用。

## 已知缺口

- detach 后窗口不自动关闭（helper 退出、`close_on_child_death=yes` 仍留窗；已写提示行）。
- 路线 A 每 tab 只画**一个** pane（活动 pane），分屏时看不到其它 pane；路线 B 由 tmux 自己画，没有这个问题。
- 4 块以上嵌套 layout、鼠标、copy mode 未验证。

## 侧栏形态的结论（2026-10-27，用户给了参考图 Harp）

用户要的侧栏是**原生 GUI 列表**：按项目分组（`Launch 4 · 1 needs you`）、每行右侧是相对时间
（`1m`/`now`/`3h`）+ 状态图标、右上角有折叠/新建按钮、窗口圆角、右侧是富文本转录。

kitty fork 给不了这个：
- kitty 的 chrome（标签栏/标题栏）是 C + OpenGL 画的固定控件，要做得加一个新的 C 侧栏控件；
- 在 kitty split 里跑 TUI（现在这版 `tmux_sidebar_tui.py`）只能画字符，做不出图标/圆角/计数徽标。

所以两条路：
- **A（推荐）** 回到原生 AppKit/SwiftTerm 应用：侧栏用 `NSTableView`（分组 + 右对齐时间 + 图标 +
  计数），右边是 SwiftTerm。tmux control-mode 的管线在 Swift 版和 Python 版都已验证过。
- **B** 在 kitty 里写 C 侧栏控件（wk 级工作），才能保留 kitty 的渲染与 fork 架构。

## 验收记录（2026-10-27，Kitty 0.49.2 fork）

| 里程碑 | 结论 | 证据 / 缺口 |
|---|---|---|
| M0 身份隔离 | ✅ | `gmux.app` + bundle id `net.kovidgoyal.gmux`，独立 config/cache；原版 kitty 不受影响 |
| M1 control mode 客户端 | ✅ | `python3.12 -m kitty.tmux_control --selftest` → 8/8 |
| M2 渲染 + 输入 | ✅（单 pane） | 实时输出 / 中文 / vim / resize / DA 只答一次；`--selftest` 3 项 pass；路线 B 下渲染交给 tmux |
| M3 layout 映射 | ⚠️ 部分 | window↔tab ✅（双 window → 2 tab，tab2 显示 `WINDOW-ONE`，状态栏 `0:bash- 1:bash*`）；pane↔kitty split ❌；动态 1 秒同步 ❌（tab 只在启动时建）；崩溃后任务仍在 ✅ |
| M4 状态 | ⚠️ 部分 | OSC 7501 → tmux window 名（`● permission bash`）✅；blocked 桌面通知 ✅；kill -9 后 ~3s 回落 ✅；独立侧栏 ❌；未读/跳转 ❌ |
| M5 验收 | ✅ | 本节 + `docs/plan.md`、README 已记录未达标项 |

未达标的两项（M3 的 pane↔split、M4 的侧栏）都需要 kitty C 侧支持，理由见“三条挡路的实测结论”。
构建：`python3.12 setup.py build`（需 go / pkg-config / simde / libxxhash，见 M0 笔记）。

## 复现命令

```sh
cd kitty && PATH=/opt/homebrew/bin:$PATH python3.12 setup.py build
python3.12 -m kitty.tmux_control --selftest      # M1
python3.12 -m kitty.tmux_child --selftest        # M2 桥接
GMUX_TMUX_SESSION=demo GMUX_TMUX_SOCKET=default \
  kitty/launcher/gmux.app/Contents/MacOS/gmux     # 真窗口
```
