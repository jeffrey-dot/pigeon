# M2 验收记录（多客户端与健壮性）

> 规则同前：能自动化的用测试卡住，只能看的用截图或手测；没验到的写“未验”。

## 1. 自动化

从仓库根目录（`cargo` 在 `~/.cargo/bin`；`swift test` 需要完整 Xcode）：

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 79 单元 + 11 ABI + 27 宿主端到端 = 117 passed
cargo build --release --bin pigeon-host            # 1,267,360 B（1.21 MiB ≤ 8 MiB）
cargo build --lib --no-default-features            # 通过（库本体不引用 tokio/portable-pty）
cbindgen --config core/cbindgen.toml --crate pigeon-core \
         --output /tmp/pg-m2.h core && diff -q include/pigeon_core.h /tmp/pg-m2.h   # 无差异
swift build && swift test                          # 通过 / 68 passed
tools/vt-parity/run.sh                             # 8/8 ok（tmux 3.8，80x24）
```

新增覆盖：

| 面 | 用例 |
|---|---|
| 尺寸策略 | `size_policy_smallest_shrinks_to_the_smallest_client`（A 100×30 → B attach+60×20 ⇒ 两边都收到 `size_state{60,20,smallest,2}`，`stty size` 也是 `20 60`）、`size_policy_latest_gives_the_box_back_to_the_typing_client`（B 抢到尺寸后 A 打字 ⇒ 弹回 100×30）、`size_policy_locked_ignores_the_newcomer`（后 attach 的请求被忽略，但仍广播当前 `source`/`clients`） |
| `size_state` | 每次尺寸变化（含被策略否决的请求）都会广播 `pane_id/cols/rows/source/clients`；`hello_ack.client_id` 现在是真实的每连接 id（之前硬编码 1） |
| 滚动回看 | `term::tests::scrolled_output_lands_in_history_oldest_first`（历史最旧在前、行宽满 cols）、`session::tests::snapshot_carries_history_when_the_pane_has_scrollback`（resize 不丢历史；80×24 喂 40 行 → 17 行历史）、`session::tests::a_wide_pane_sends_the_newest_history_that_fits_one_frame`（500 列时按单帧预算裁旧留新）、`scrollback_history_reaches_the_client_in_the_snapshot`（真宿主 + 真 PTY：`seq 1 40` 后重新 `attach`，历史里能看到 1/2、屏幕上是 40） |
| 滚动回看（客户端） | `PaneGridTests`：`clampedOffset` 上下界与空历史、`canScroll` 对 alt 屏/无历史返回 false、`wheelLines` 触控板像素累积成整行（12+12 px / 18 px = 1 行，余 6 px） |
| 宿主断连 | `HostRecoveryTests.testAClosedHostSocketIsReported`（真实 unix socket 对端消失 ⇒ `onClose` 在 5s 内触发，这就是 `kill -9` 在客户端眼里的样子）、`testLaunchStartsARealProcess`（真的把进程拉起来并等它写下标记文件）、`testFindSkipsMissingBinaries` / `testLauncherPrefersTheExplicitPathThenTheWorkingDirectory`（候选顺序）、重试策略 4 条（3 次给不给按钮、放弃时机、banner 文案、`startCommand`） |
| 版本拒绝 | `bad_version_is_rejected_with_hello_error`（宿主侧）；客户端把 `hello_error` 的 `reason`/`detail` 显示到提示里并停止重试 |
| zoom | `layout::tests::zoom_gives_one_pane_the_whole_window_and_unzoom_restores_the_tree`（zoom 后该 pane rect = 整窗、焦点跟过去、unzoom 后嵌套比例逐字段还原、未知 pane 报 false）、`closing_the_zoomed_pane_drops_the_zoom`、`zooming_a_pane_gives_it_the_whole_window_and_unzoom_restores_the_split`（真宿主：`layout.zoom` 下发、`size_state` 变成整窗 80×24、unzoom 后叶子顺序不变、未知 pane 回 `no_such_pane`）、`zoom_messages_round_trip`（协议往返） |
| VT 对账 | `tools/vt-parity/`：8 条语料（basic/cursor/erase/wide/scroll-region/alt-screen/insert-delete/wrap）与 tmux `capture-pane` 逐行一致 |

## 2. 这轮抓到的真 bug（都已修）

| 问题 | 表现 | 位置 |
|---|---|---|
| tab 占位符漏到快照 | alacritty 在制表符跳过的空白串首格存了一个 `'\t'`，`convert_row` 原样透传，客户端会去画一个 tab 字形（多数终端渲染器把 `\t` 当空白，我们没做） | `core/src/term/alacritty.rs`：控制字符统一归一成空格；`term::tests::a_tab_advances_to_the_next_stop_without_writing_a_glyph` 卡住 |
| 滚动回看只能看到 attach 时的历史 | `history` 只随 `attach` 的 `snapshot` 下发（增量帧不带历史），客户端缓存的历史与当前屏幕之间会断开 | 客户端回看前重新 `attach` 取历史并冻结这一屏（`PaneView.pendingScroll`/`frozenScreen`），协议写清这条约束（`docs/protocol.md` §3.3） |
| 宽 pane 的快照会超帧 | 1000 行 × 500 列 ≈ 10MB > 8MB 单帧上限，`attach` 会直接失败 | `session::history_window` 按预算裁旧留新 |
| `pg_msg_layout_focus` 从 C 头里消失 | 加 `pg_msg_layout_zoom` 时漏了它自己的 `#[no_mangle]`，cbindgen 不再导出，Swift 直接编译失败 | `core/src/ffi/msg.rs`；头文件重新生成后 diff 干净 |

## 3. 未验 / 已知缺口

自动化和 GUI 是两回事，下面每一条都有上面列出的自动化覆盖，但**没有真人动鼠标键盘**：

| 项 | 自动化到哪一步 | 还缺什么 |
|---|---|---|
| 两个窗口看同一会话，尺寸正确 | 协议层用两个 socket 客户端验过三种策略 | 真开两个 `Pigeon.app` 窗口看一眼；`size_state` 在侧栏 age 位显示 `2× latest` 也只是纯逻辑单测过 |
| `kill -9` 宿主 → 提示 + 重开 | 断连检测、重试策略、拉起进程都单测过 | 真 `kill -9` 后看覆盖层文案与自动重连；第一次连接失败（宿主没起）时的路径同样未手测 |
| 滚轮 / PageUp·PageDown / 滚动条 | offset 夹取、alt 禁用、像素累积单测 | 触控板真的滚一下；`alt` 屏里（vim）确认键还归程序 |
| ⌘⇧↩ zoom | 消息与布局端到端 | 真按一次快捷键 |
| 回看上限 | 宿主 `SCROLLBACK = 1000` 行 | **与 M2 验收里写的“默认 1 万行”不符**：1 万行 × 80 列 ≈ 12MB，超过 8MB 单帧上限。要么协议加分块历史，要么把上限调大并接受 attach 变慢——留给后续定 |
| 多客户端首次 attach 决定策略 | `locked`/`smallest` 用两个客户端验过 | 客户端目前不发送 `size_policy`（默认 `latest`），要在 UI 里选策略才有意义 |

另：`three_panes_survive_a_disconnect_and_restore_layout_and_screens` 在机器很忙时（并行跑 `swift build`
等）偶发超时，串行/单跑稳定；这是测试的时序敏感，不是功能回归。

## 4. CI

本轮改动尚未推 CI（本地 macOS 全绿）。推上去后应看两处：`rust` 作业的 `cargo test`（含 3 条新尺寸策略用例）与
`swift` 作业的 68 个用例；`tools/vt-parity/run.sh` 没进 CI——golden 来自本机 tmux 3.8，Linux 上 tmux 版本不同
可能给出不同渲染，手工跑更诚实。
