# M3 验收记录（侧栏）

> 规则同前：能自动化的用测试或脚本卡住，只能看的用截图；没验到的写“未验”。

## 1. 自动化

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 66 单元 + 11 ABI + 21 宿主 = 98 passed
cargo build --release --bin pigeon-host            # 1,234,288 B（1.18 MiB ≤ 8 MiB）
cbindgen --config core/cbindgen.toml --crate pigeon-core \
         --output /tmp/pg.h core                   # 与 include/pigeon_core.h 无差异
cargo build --lib --no-default-features            # 通过（库本体不引用 tokio/portable-pty）
swift build && swift test                          # 通过 / 40 passed
```

新增覆盖：

| 面 | 用例 |
|---|---|
| OSC 7 / 标题 | `osc7_sets_the_cwd_and_bad_urls_are_ignored`（file:// + `%20` 正例；http、`%zz`、控制字符三条反例保留旧值）、`osc0_and_osc2_set_the_title`（含控制字符的标题被丢弃） |
| 状态流 | `status_is_broadcast_once_per_change_and_ack_stops_repeats`（working → done，`msg` base64 解出 “OK”；ack 后不再重复）、`sessions_carry_the_active_pane_and_its_title`、`sessions_refresh_when_the_active_pane_title_changes`（OSC 0/2 → 宿主重发 sessions）、`acked_status_is_not_replayed_after_reconnect`、`clearing_a_status_broadcasts_idle` |
| 侧栏逻辑 | `blocked` 优先且按持久化顺序、`nextNeedsYou` 回绕、协议状态字符串映射、`relativeTime` 四档（`now`/`1m`/`1h`/`1d`）、`statusGlyph` 覆盖五个状态（`idle` 为空）、`ageText` 未上报时为空、`subtitle(status:since:)` = `Needs you · 2m` / `Idle · now` |
| 持久化 | `SidebarStore` 往返 + 原子写、默认路径在 `~/.config/pigeon`、损坏文件回退为空状态 |

## 2. GUI（真键盘 / 真鼠标，`tools/m3-gui-check.sh`）

四个 session：`A` 组两个（同 cwd，用来演示组内拖拽）、`B` 组的 shell 隔 2s 打印 `done`（保持未读）、
`C` 组的 shell 启动即打印 `blocked:kind=permission`。空态在创建任何 session 之前截；
浅/深两套都跑（`PIGEON_APPEARANCE=light|dark` 固定外观，不动系统设置）。真实输出：

```text
sessions created: 4
sidebar (light): sidebar sessions=4 needs=2
sidebar width: sidebar width=272
apps: sessions apps=1:sh 2:beta 3:done.sh 4:blocked.sh
hot switch: dark + light captured from one process
Cmd-J focus messages: 0 -> 1 (ack: 1)
selected before close: selected=4
Cmd-Shift-W Close Session: close_pane 0 -> 1
sessions after Cmd-T: 5
order (manual drag, see docs/m3-checklist.md §5): []
reopened: sidebar sessions=4 needs=0
state file: …/config/sidebar.json (82 bytes)
sidebar (dark): sidebar sessions=4 needs=0
```

拖拽排序在这个环境**无法自动化**：合成的鼠标事件不会让 AppKit 启动拖拽会话——
`SidebarViewController.pasteboardWriterForRow` 里的 `drag start` 调试行，在一次完整跑里出现 **0 次**
（`grep -c 'drag start' /tmp/pigeon-m3-*/app-light.log` → `0`），所以脚本只打印一句提示，不再伪造 `reordered` 截图。
持久化机制本身有用例与手验：`acceptDrop` → `onReorder` → `SidebarStore` 原子写（`SidebarStoreTests`），
更早一次人手拖拽把 `sidebar.json` 写成 `{"order":[1,3,2],…}` 并重开生效（见 §5）。

交互证据：[右键菜单](images/m3/menu-light.jpg)（关闭 / 标记已读 / 重命名）、
[分组右键](images/m3/group-menu-light.jpg)（`Rename Group…`）、
[关窗重开](images/m3/reopen-light.jpg)（分组、未读、排序原样回来）。
截图坐标按 §7.1 的几何算：56pt 头部 + 4pt 顶部留白 + 组行 42 + 会话行 31。

### 2.1 与 harp 的逐项对照（criterion 2）

harp 出处见 `docs/ui.md` §7（`VT` = `app/src/workspace/view/vertical_tabs.rs`，`TL` = `app/src/util/traffic_lights.rs`）。

| 项 | harp（出处 / 取值） | 我们的实现 | 截图 |
|---|---|---|---|
| 侧栏宽度 | `VT:9110` `SIDEBAR_WIDTH` 272 | 默认 272，`splitView.setPosition(272)` 真正落位（`PIGEON_DEBUG` 打出 `sidebar width=272`），可拖 200–360 | [sidebar-light](images/m3/sidebar-light.jpg) |
| 侧栏底色 | `VT:9209` `SIDEBAR` `#e8e8eb` / `#232326`；`GLASS_TINT` 半透明罩 | `SidebarBackgroundView` 动态色填同样两值（不叠加玻璃模糊，见 §7.5） | [sidebar-light](images/m3/sidebar-light.jpg) / [sidebar-dark](images/m3/sidebar-dark.jpg) |
| 窗口 chrome | `TL:64` 左侧 64pt 红绿灯区、标题栏透明；`view.rs:581` 红绿灯中心 28pt、与侧栏按钮同排 | `.fullSizeContentView` + `titlebarAppearsTransparent` + `titleVisibility = .hidden`；侧栏顶到窗口顶端；折叠/新建按钮按**原生红绿灯中心**对齐（macOS 自己摆红绿灯，我们跟随它的行） | [sidebar-light](images/m3/sidebar-light.jpg) / [sidebar-dark](images/m3/sidebar-dark.jpg) |
| 组头项目名 + 条数 | `VT:9123` 11.5 Semibold `SECTION_NAME`；`VT:10092` 11.5 `SECTION_COUNT`；首字母大写 | 同名 token；`A 2` / `B 1` / `C 1` | [sidebar-light](images/m3/sidebar-light.jpg) |
| 组头 `needs you` | `VT:10101` 12pt `NEEDS_TEXT`；单数 `1 needs you`、复数 `N need you` | `SidebarTheme.needsLabel` 同文案同色 | [sidebar-light](images/m3/sidebar-light.jpg)（B、C 组） |
| 行高 / 行距 | `VT:9117` 30、`VT:9116` 1 | `rowHeight 30` + `intercellSpacing 1` | [sidebar-light](images/m3/sidebar-light.jpg) |
| 组间距 | `VT:9115` 间距 14 + `VT:9122` 组头 28 | 组行 42：14pt 空档在**上**、标签占下面 28pt（组头与首个行之间为 0） | [sidebar-light](images/m3/sidebar-light.jpg)（B/C 组上方的空档） |
| 行圆角 / 选中 | `VT:9120` 8、`VT:9204` `WHITE`、`SELECTED_EDGE` 0.5、`SELECTED_SHADOW`（`VT:9214`） | `SidebarRowView` 自绘胶囊：8pt、动态色、0.5pt 描边、投影；`selectionHighlightStyle = .none`；窗口失焦时才用 `QUIET_SELECTED`（§2.2 取样 `#ffffff` / `#3a393e`） | [sidebar-light](images/m3/sidebar-light.jpg) / [sidebar-dark](images/m3/sidebar-dark.jpg) |
| 悬停 | `VT:9210` `HOVER` 黑 5% / 白 6% | `SidebarRowView` 的 tracking area 用同一 token；脚本把指针移到行上再截 | [hover-light](images/m3/hover-light.jpg) |
| 行内缩进 / 右列 | `VT:10031` 左 12 / 右 8，`VT:9114` 列表左右 10 | 标题左 22pt、状态图标右缘 18pt、元素间距 8pt | [sidebar-light](images/m3/sidebar-light.jpg) |
| 标题字号 / 字重 | `VT:9120` 13、未读 Semibold、`VT:9193` `INK` | 13pt / Semibold / `SidebarTheme.ink` | [sidebar-light](images/m3/sidebar-light.jpg)（`done.sh` 行） |
| 时间 | `VT:8568` 10pt；颜色 `VT:8564` → `needs_you_color()` `#E8A33D`；`VT:9390` `now/5m/3h/2d` | 10pt；blocked 行用 `needsAccent`（`#E8A33D`），其余 `GRAY`；`SidebarModel.relativeTime` 同四档 | [sidebar-light](images/m3/sidebar-light.jpg) |
| 状态图标 | `VT:9544-9604` 15pt 字形 / 16pt 槽；needs 用 `ICON_NEEDS_RING`/`ICON_NEEDS_DOT`（`VT:9222-9223`） | SF Symbols：`circle.dashed` / `exclamationmark.circle` / `checkmark.circle.fill` / `checkmark` / 无；needs 用 `iconNeedsRing`（浅 `#c98314`、深 `#e8a33d`） | [sidebar-light](images/m3/sidebar-light.jpg) / [jump-light](images/m3/jump-light.jpg) |
| 未读 | `VT:9849` 标题 Semibold + 实心 done 盘 | 同上，加 `docs/ui.md` §2.1 要求的行首 5pt 圆点（harp 没有这个点） | [sidebar-light](images/m3/sidebar-light.jpg)（`done.sh` 行） |
| 顶部按钮 | `VT:10146-10162` 28×28、圆角 7、图标 17、`GRAY`、悬停 `HOVER` 填充 | `SidebarHeaderButton`：28×28、自绘 7pt 圆角悬停填充、图标 17、右对齐 14pt | [sidebar-light](images/m3/sidebar-light.jpg) |
| 右栏头部 | `VT:9125` 40pt、12.5 Semibold `INK` + 12.5 `GRAY` 路径（行首截断）+ 12 `GRAY` 状态、左 32 / 右 20、0.5 `DIVIDER` | `SessionHeaderView` 同值；下边线是 0.5pt `DividerView`（`divider`，浅 `#ececef` / 深 `#3a3a3c`，跟外观），侧栏收起时左内边距 84（`VT:9127`） | [sidebar-light](images/m3/sidebar-light.jpg) / [jump-light](images/m3/jump-light.jpg) |
| 空态 | harp 没有 | `docs/ui.md` §2.3：“No sessions yet” + New Session | [empty-light](images/m3/empty-light.jpg) / [empty-dark](images/m3/empty-dark.jpg) |
| 右栏卡片 | `view.rs:579` 圆角 10；`view.rs:22245` 0.5pt `CARD_EDGE`；`view.rs:22247` `CARD_SHADOW` 投影（下移 4、模糊 16）；`view.rs:22258-22262` 上/右/下各 8pt 外缩 | `CardView`：内层 `cornerRadius 10` + `masksToBounds`（防终端底色破角）、外层投影，上/右/下各 8pt，左边贴分隔线；终端内容再左缩 32pt；**卡片底色 = 主题背景**（`terminalBackground`，浅 `#ffffff` / 深 `#050505`），头部用 `ink`/`gray`（跟外观） | [sidebar-light](images/m3/sidebar-light.jpg) / [jump-light](images/m3/jump-light.jpg) |
| 终端默认底/字 | `default_themes.rs:262` `dark_theme()`（bg `#050505`、fg `#ffffff`）/ `:275` `light_theme()`（bg `#ffffff`、fg `#111111`）；卡片背景 `view.rs:22241` = 主题背景 | `SidebarTheme.terminalBackground` 浅 `#ffffff` / 深 `#050505`，`terminalForeground` 浅 `#111111` / 深 `#ffffff`；只作用于“没有显式颜色”的格子，ANSI 16/256 色不动（`CoreTextRenderer.defaultColors(for:)`） | [sidebar-light](images/m3/sidebar-light.jpg) / [sidebar-dark](images/m3/sidebar-dark.jpg) |
| 外观切换 | — | 脚本在**同一个进程**里按 ⌥⌘A 触发 `NSApp.appearance` 翻转（`PIGEON_DEBUG` 下的调试菜单项），前后各截一张；`CardView` 在 `viewDidChangeEffectiveAppearance` 里重解析 layer 色，侧栏/未读点由 `SidebarRenderingTests` 断言 | [hot-switch-dark](images/m3/hot-switch-dark.jpg) → [hot-switch-light](images/m3/hot-switch-light.jpg) |

### 2.2 像素抽查（同一份截图取样，`/tmp/pxbin` 读 JPEG）

| 外观 | 侧栏 (20,320) | 卡片 (700,300) | 选中胶囊 (130,143) | 期望（`docs/ui.md` §7） | 结论 |
|---|---|---|---|---|---|
| 浅色 | `#e8e7ec` | `#ffffff` | `#ffffff` | 侧栏 `#e8e8eb`、卡片/终端 `#ffffff`、选中 `WHITE` | 一致（JPEG 压缩 ±3） |
| 深色 | `#232227` | `#050505` | `#3a393e` | 侧栏 `#232326`、卡片/终端 `#050505`、选中 `#3a3a3d` | 一致（JPEG 压缩 ±3） |

选中胶囊这一列同时验证了 `QUIET_SELECTED` 的语义：只有窗口不是 key window 时才用 `#f4f4f5` / `#2f2f32`，
脚本截图前会把 app 置前，所以读到的是 `WHITE`。

取色方式：`screencapture -R`（窗口级）→ `sips` 转 JPEG → `CGImageSource` 读像素。这组数值就是“一半白一半黑”修好之后的证据：
浅色下侧栏与卡片同属浅色（`#e8e8eb` / `#ffffff`），深色下同属深色（`#232326` / `#050505`）。

## 3. 产品调整（用户要求，2026-10-28）

- **行标题改成活跃 pane 的应用名**（`sh` / `zsh` / `pi` / `vim`），不再显示 `session N`；副标题只留状态与相对时间。
- **分组不再折叠**：去掉点击折叠与 `collapsed` 交互；分组行只显示“项目名 + 条数”。
- **分组可重命名**：右键分组 → `Rename Group…`，写入 `sidebar.json` 的 `renamedGroups`，重开生效。
- **按 `docs/ui.md` + 参考图对齐行版式**（2026-10-28 晚）：行从两行改成单行——标题左、右侧固定列放
  `now/1m/1h/1d` + 状态字形（`◐ ◷ ✓ !`，`idle` 无字形），行高 46 → 26；分组头右对齐橙色 `N needs you`
  （与组内 blocked 数一致）；未读才加粗 + 行首圆点，去掉整行染色；相对时间每分钟自动刷新。
  `tools/m3-gui-check.sh` 的点击步长同步 46 → 26。
- **副标题的去处**（2026-10-28 用户确认）：行保持参考图的单行（标题 + 时间 + 图标），
  `Working · 1m` / `Needs you · 2m` 这句话改由右栏头部徽标承担（`SessionHeaderView`），文案仍来自
  `SidebarModel.subtitle`。
- **侧栏顶部工具条 + 空态**（ui.md §2.3）：侧栏顶部两个 SF Symbol 按钮（折叠 / 新建）、⌘B 折叠、
  没有 session 时显示 “No sessions yet” + “New Session”。
- **右栏头部**（ui.md §3）：`会话名 + cwd（~ 缩写）+ 状态徽标`，与侧栏同一份状态；没有 OSC 7501 记录时徽标留空。

## 4. 这轮抓到的真 bug（都已修）

1. **侧栏选中触发无限递归崩溃**：`update()` 里 `selectRowIndexes` 触发 `selectionDidChange` → `onSelect` → `refreshSidebar` → `update()`；
   崩溃报告显示 1182 层递归后栈溢出。修法：程序化更新期间 `suppressSelection`。
2. **状态不进侧栏**：客户端只 attach 选中 session 的 pane，其它 session 的 `status` 事件被宿主丢弃。改为 attach 每个 session 的活跃 pane。
3. **后接入的客户端看不到已有状态**：宿主在 `attach` 时补发该 pane 的最新 `status`（`Cmd::LatestStatus`）。
4. **右键菜单吞掉下一次点击**：脚本里右键后必须先按 Esc 关闭菜单，否则折叠/拖拽的点击都落在菜单上。
5. **group 行不可选**：`NSTableView` 的 group 行不参与选择，折叠永远不触发；改为普通行 + 选择即折叠。
6. **拖拽接收未注册**：补 `registerForDraggedTypes` 与拖拽源 mask（见下节的限制）。
7. **分组顺序每次启动都在变**：`sidebarSessions()` 直接遍历 `Dictionary`（顺序随机），分组按“首次出现”排序，
   于是同一份状态两次启动可以 a/b/c 也可以 b/c/a；脚本的行坐标也跟着漂。改为按 session id 排序后再分组。
8. **blocked 被算成未读**：`applyStatus` 把所有非 idle 状态都记成 unread，blocked 行因此出现蓝点，
   与 `docs/ui.md` §5（未读只来自 `done`/`error`，blocked 用橙色标记）不符；改为只对 `done`/`error` 计未读。
9. **“没有上报”被画成 “Idle · now”**：右栏徽标和行的相对时间都用 `lastSeen`，而没有 OSC 7501 记录的 session
   从来不会更新它，于是界面上写着“Idle”，时间其实是这一行刚出现的时间。改为 `SidebarSession.reported`
   （来自 `status.seq > 0`）：没有记录就不画徽标、不画时间。

10. **卡片里的终端容器算出零高度**：卡片改成 `header + 内容` 的垂直 `NSStackView` 后，`contentBox` 没有固有高度，
   栈把它排成 0 —— 终端视图拿不到尺寸，pane 里只有卡片底色。修法：改成显式约束（header 贴顶固定 40pt，
    内容接在 header 下方撑满），终端内容再在卡片内左缩 32pt。证据：修前日志里有 `grid pane=2 2x1 cursor=0,0`。
11. **`syncGrid()` 把退化尺寸发给宿主，导致整屏内容被 resize 丢弃**：`PaneView.syncGrid()` 原来是
    `max(2, width/cols)`，视图还没布局时也会上报 **2×1**；宿主据此把该 pane 的终端 resize 到 2×1，
    内容就此丢掉。重排回来的尺寸只是“格子变大”，**不会把旧内容变回来**；纯 shell 会因 SIGWINCH 重画提示符，
    而空闲的 TUI（这里的 `pi`）不会重绘，于是 `π - core` 那个 pane 一直空白、光标停在第 30 行。
    证据：`grid pane=2 97x36 cursor=0,30 filled=0 lastRow=-1`（整屏 0 个字形），同一时刻 `pane=1 filled=26`；
    手动把窗口缩小再放大触发 SIGWINCH 后恢复成 `filled=1421 lastRow=35`。修法：判定抽成纯函数
    `PaneView.gridSize(for:cellSize:)`，`<2` 列 / `<1` 行 / 零 cellSize 一律返回 `nil`，不上报；
    `PaneGridTests` 有四个反例 + 一个正例。
12. **点侧栏行不切换右栏内容**：协议里 `attach` 同时是“这个窗口跟随哪个 session”的声明（宿主收到后把
    `state.session` 切过去，并回推 `layout` + `snapshot`）。客户端启动时给每个 session 的活跃 pane 都 attach
    （为了侧栏状态），宿主最后跟的是**最后 attach 的那个**；而 `sendAttach` 有“attached 过就不再发”的去重，
    点别的行时什么都没发出去 → 没有该 session 的 `layout`，右栏保持原样。证据：修前只有 `select id=3`，
    没有对应的 `layout session=3`。修法：`sendAttach(_:force:)`，`select()` 对目标 session 强制重 attach；
    修后日志成对出现 —— `select id=3 activePane=3 panes=[3]` → `layout session=3 selected=3 panes=[3]`。

## 5. 未验

| 项 | 说明 |
|---|---|
| 拖拽排序（自动化） | 合成鼠标事件无法启动 AppKit 拖拽：`SidebarViewController.pasteboardWriterForRow` 里的 `drag start` 调试行，在一次完整跑（`PIGEON_DEBUG=1 tools/m3-gui-check.sh`）的 app 日志里是 **0 次**（`grep -c 'drag start' /tmp/pigeon-m3-*/app-light.log`）。`cliclick` 的 `dd/dm/du`、单进程连发、先 `m:` 定位都试过，落点仍停在起始行。机制本身可用：`acceptDrop` → `onReorder` → `SidebarStore` 原子写，`SidebarStoreTests` 覆盖往返；更早一次人手拖拽把 `sidebar.json` 写成 `{"order":[1,3,2],…}` 并重开生效 |
| 拖拽排序（人手） | 需要真人拖一次才能给出可视化证据；脚本不产出 `reordered` 截图（旧图已删） |
| ~~右键 Close Session~~ | 已稳定：`close_pane 0 -> 1` |
| 悬停 `×` 关闭 | ui.md §2.1 提到，未实现；关闭走右键菜单 / ⌘⇧W |
| 相对时间的“最后输出时间” | 未上报的行留空（§4.9）。harp 用终端最后一条命令的时间（`VT:9410` `age_from`），我们要宿主上报 pane 活动才做得到（M4） |
| harp 的玻璃材质模糊 | 只取 `SIDEBAR` 不透明底色，不叠 `GLASS_BLUR_RADIUS` 30（自绘窗口背景，AppKit 下用系统材质，见 `docs/ui.md` §7.5） |
| 输入法 / 手速 | M6 |
| SSH / 多主机分组 | M4 |

## 6. CI（GitHub Actions，真跑）

代码提交 `fb0576c`（HEAD；浅/深同色系 + 文档收尾）的 CI 五作业 success：

<https://github.com/jeffrey-dot/pigeon/actions/runs/40664325916>

| 作业 | 结果 |
|---|---|
| `docs` | success（相对链接检查） |
| `rust (ubuntu-latest)` | success（`cargo test` 66 + 11 + 21 = 98 passed） |
| `rust (macos-latest)` | success |
| `header` | success（cbindgen `diff -u` 为空） |
| `swift` | success（`swift test` 42 passed） |

同一提交首跑时 `rust (macos-latest)` 红过一次：`acked_status_is_not_replayed_after_reconnect` 用 `sleep(300ms)` 等重连后的补发，
在负载高的 runner 上会抢跑（`core/tests/host.rs:1173`），属既有时间敏感用例；`gh run rerun --failed` 后五作业全绿。

本机同一提交的全量输出：

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 66 + 11 + 21 = 98 passed
swift build && swift test                          # Build complete / 42 passed
cargo build --release --bin pigeon-host            # 1,234,288 B（1.18 MiB ≤ 8 MiB）
cbindgen … --output /tmp/pg4.h && diff -q …        # header: no diff
tools/m3-gui-check.sh                              # 见 §2 转录（浅/深 + 热切换，窗口级截图）
```
