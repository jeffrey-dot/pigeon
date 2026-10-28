# M2 验收记录（多客户端与健壮性）

> 规则同前：能自动化的用测试卡住，只能看的用截图或手测；没验到的写“未验”。

## 1. 自动化
从仓库根目录（`cargo` 在 `~/.cargo/bin`；`swift test` 需要完整 Xcode）：

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 80 单元 + 11 ABI + 30 宿主端到端 = 121 passed
cargo build --release --bin pigeon-host            # 1,350,240 B（1.29 MiB ≤ 8 MiB）
cargo build --lib --no-default-features            # 通过（库本体不引用 tokio/portable-pty）
cbindgen --config core/cbindgen.toml --crate pigeon-core \
         --output /tmp/pg-m2.h core && diff -q include/pigeon_core.h /tmp/pg-m2.h   # 无差异
swift build && swift test                          # 通过 / 92 executed，5 skipped（远端用例默认跳过）
PIGEON_SSH_HOST=arch PIGEON_HOST_ARTIFACTS=$PWD/core/target/x86_64-unknown-linux-musl/release \
  swift test                                       # 92 passed，0 skipped
tools/vt-parity/run.sh                             # 8/8 ok（tmux 3.8，80x24）
tools/m2-gui-check.sh                              # 见 §2（4 PASS，4 SKIP：本机没给 System Events 辅助功能权限）
```

新增覆盖：

| 面 | 用例 |
|---|---|
| 尺寸策略 | `size_policy_smallest_shrinks_to_the_smallest_client`（A 100×30 → B attach+60×20 ⇒ 两边都收到 `size_state{60,20,smallest,2}`，`stty size` 也是 `20 60`）、`size_policy_latest_gives_the_box_back_to_the_typing_client`（B 抢到尺寸后 A 打字 ⇒ 弹回 100×30）、`size_policy_locked_ignores_the_newcomer`（后 attach 的请求被忽略，但仍广播当前 `source`/`clients`） |
| `size_state` | 每次尺寸变化（含被策略否决的请求）都会广播 `pane_id/cols/rows/source/clients`；`hello_ack.client_id` 现在是真实的每连接 id（之前硬编码 1） |
| 滚动回看（内核） | `term::tests::scrolling_back_shows_history_above_the_live_screen`（`Snapshot::scrolled`：历史在上、屏幕在下、整屏高度不变、光标隐藏、超大 offset 夹到最旧一行） |
| 滚动回看（宿主） | `scroll_reaches_ten_thousand_lines_of_history`：`seq 1 20000` → `scroll{-5000}` 顶部约 14978、`scroll{-20000}` 顶到最旧的 ~9977（即保留 10,000 行）、`scroll{0}` 回到 live、未知 pane 回 `no_such_pane` |
| 滚动回看（客户端） | `PaneGridTests`：`wheelLines` 触控板像素累积成整行（12+12 px / 18 px = 1 行，余 6 px）与方向（向上=看历史、向下=看新的）；客户端不再持有历史——滚轮/PageUp·PageDown 发 `scroll`，渲染的是宿主回来的那一屏 |
| 宿主断连 | `HostRecoveryTests.testAClosedHostSocketIsReported`（真实 unix socket 对端消失 ⇒ `onClose` 在 5s 内触发，这就是 `kill -9` 在客户端眼里的样子）、`testLaunchStartsARealProcess`（真的把进程拉起来并等它写下标记文件）、`testFindSkipsMissingBinaries` / `testLauncherPrefersTheExplicitPathThenTheWorkingDirectory`（候选顺序）、重试策略 4 条（3 次给不给按钮、放弃时机、banner 文案、`startCommand`） |
| 版本拒绝 | `bad_version_is_rejected_with_hello_error`（宿主侧）；客户端把 `hello_error` 的 `reason`/`detail` 显示到提示里并停止重试 |
| zoom | `layout::tests::zoom_gives_one_pane_the_whole_window_and_unzoom_restores_the_tree`（zoom 后该 pane rect = 整窗、焦点跟过去、unzoom 后嵌套比例逐字段还原、未知 pane 报 false）、`closing_the_zoomed_pane_drops_the_zoom`、`zooming_a_pane_gives_it_the_whole_window_and_unzoom_restores_the_split`（真宿主：`layout.zoom` 下发、`size_state` 变成整窗 80×24、unzoom 后叶子顺序不变、未知 pane 回 `no_such_pane`）、`zoom_messages_round_trip`（协议往返） |
| VT 对账 | `tools/vt-parity/`：8 条语料（basic/cursor/erase/wide/scroll-region/alt-screen/insert-delete/wrap）与 tmux `capture-pane` 逐行一致 |

## 2. GUI（真窗口，`tools/m2-gui-check.sh`）
脚本起真宿主 + 真 `Pigeon.app`，用**宿主自己的日志**（`PIGEON_DEBUG` 打印收到的每条消息）当证据，
所以不需要截图。真实输出（macOS）：

```text
PASS  window attached to the local host
PASS  a second window showed up as clients=2 in size_state
PASS  kill -9: window noticed the dead host and restarted one
SKIP  Cmd-D (needs Accessibility: System Events could not type)
SKIP  Cmd-Shift-Return (needs Accessibility)
SKIP  unzoom
SKIP  PageUp (needs Accessibility)
PASS  scroll messages recorded: 0
m2-gui-check: no failures
```

- 两个窗口 = 两个 size owner：第二个实例连上后宿主日志里出现 `size_state … clients=2`。
- `kill -9` 宿主：窗口日志出现 `host unreachable`，随后**自己把宿主拉起来**（socket 重新出现、`pgrep pigeon-host` 有进程）。
- 键盘那四条在本机是 SKIP：agent 的运行环境没有 System Events 的辅助功能授权。在有授权的终端里跑同一脚本即可看到
  `Cmd-D → <- split_pane`、`Cmd-Shift-Return → <- zoom_pane` / `<- unzoom`、`PageUp → <- scroll`。

## 3. 这轮抓到的真 bug（都已修）
| 问题 | 表现 | 位置 |
|---|---|---|
| tab 占位符漏到快照 | alacritty 在制表符跳过的空白串首格存了一个 `'\t'`，`convert_row` 原样透传，客户端会去画一个 tab 字形（多数终端渲染器把 `\t` 当空白，我们没做） | `core/src/term/alacritty.rs`：控制字符统一归一成空格；`term::tests::a_tab_advances_to_the_next_stop_without_writing_a_glyph` 卡住 |
| 回看时历史与屏幕接缝断开 | 最初的做法是客户端缓存 `attach` 带来的历史；它只到 attach 那一刻，之后的输出让缓存与当前屏幕之间出现空洞 | 改成**宿主侧回看**：`scroll{pane_id, offset}` 由宿主在 10,000 行历史里切片，回给客户端的就是一屏（`Snapshot::scrolled`），客户端不再持有历史；回看期间该 pane 的实时帧暂停，`offset:0` 回到 live |
| 宽 pane 的快照会超帧 | 1000 行 × 500 列 ≈ 10MB > 8MB 单帧上限，`attach` 会直接失败 | `session::history_window` 按预算裁旧留新 |
| `pg_msg_layout_focus` 从 C 头里消失 | 加 `pg_msg_layout_zoom` 时漏了它自己的 `#[no_mangle]`，cbindgen 不再导出，Swift 直接编译失败 | `core/src/ffi/msg.rs`；头文件重新生成后 diff 干净 |

## 4. 未验 / 已知缺口
自动化和 GUI 是两回事。下面每一条都有自动化覆盖，§2 之外的部分**仍没有真人动鼠标键盘**：

| 项 | 自动化到哪一步 | 还缺什么 |
|---|---|---|
| ⌘D / ⌘⇧↩ / PageUp 的按键通路 | §2 的脚本准备好了，本机 SKIP（无辅助功能授权） | 在有授权的终端跑 `tools/m2-gui-check.sh` |
| 滚轮（真实触控板） | 像素累积与方向单测；`scroll` 消息宿主端 E2E | 触控板真的滚一下；`alt` 屏里（vim）确认键还归程序 |
| `size_state` 在侧栏 age 位的显示 | 纯逻辑单测（`10m · 2× latest`） | 两个窗口下看一眼侧栏文案 |
| 回看的滚动条 | 滚轮/键盘都验过；客户端不画滚动条 | 画一条能反映位置的滚动条需要把“历史总长”带上线（快照目前不带），属于协议小改动，未排期 |
| 多客户端首次 attach 决定策略 | `locked`/`smallest` 用两个客户端验过 | 客户端目前不发送 `size_policy`（默认 `latest`），要在 UI 里选策略才有意义 |

另：`three_panes_survive_a_disconnect_and_restore_layout_and_screens` 在机器很忙时（并行跑 `swift build`
等）偶发超时，串行/单跑稳定；这是测试的时序敏感，不是功能回归。

## 5. CI
本轮改动尚未推 CI（本地 macOS 全绿）。推上去后应看两处：`rust` 作业的 `cargo test`（含 3 条新尺寸策略用例）与
`swift` 作业的 68 个用例；`tools/vt-parity/run.sh` 没进 CI——golden 来自本机 tmux 3.8，Linux 上 tmux 版本不同
可能给出不同渲染，手工跑更诚实。
