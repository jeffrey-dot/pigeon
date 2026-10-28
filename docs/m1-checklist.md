# M1 验收记录（分屏）

> 规则同 M0：能自动化的用测试卡住，只能看的用截图；没验到的写“未验”。

## 1. 自动化

从仓库根目录：

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 62 单元 + 11 ABI + 12 宿主端到端 = 85 passed
cargo build --release --bin pigeon-host            # 1,201,200 B（1.15 MiB ≤ 8 MiB）
cbindgen --config core/cbindgen.toml --crate pigeon-core \
         --output /tmp/pg.h core                   # 与 include/pigeon_core.h 无差异
cargo build && swift build                         # 客户端链接 core/target/*/libpigeon_core.a
swift test                                         # 22 个用例
```

新增覆盖：

| 面 | 用例 |
|---|---|
| 布局树 | `layout::tests`（10 个）：左右/上下、三格、四格嵌套不重叠且面积和 = 1、关闭归并、关闭唯一 pane 变空、比例按最小尺寸夹取、`set_ratio` 夹取、焦点循环含回绕、关闭焦点后迁移、缺失 pane 报错、`split_of` 找分隔线、`describe` 与原树几何一致 |
| C ABI | `layout_splits_closes_and_focuses`（split 返回新 pane、rects 各 0.5、resize 夹到 0.9、focus 只能指向活 pane、close 后兄弟占满、split_of）、`layout_message_exposes_rects_and_splits`（layout 消息经 ABI 给出 rects/splits/focus） |
| 协议 | `SplitPane`/`ResizeSplit`/`Focus`/`Layout` 往返 + 两层树与 `focus`；反例：`axis="x"`、`ratio="half"`、`zoom_pane`/`unzoom` → `unsupported` |
| 宿主端到端 | `split_makes_three_live_panes`（三个 pane 各自 echo 自己的 marker）、`three_panes_survive_a_disconnect_and_restore_layout_and_screens`（叶子顺序/split id+axis+ratio/focus/每 pane 逐 cell 快照都一致）、`closing_a_pane_merges_it_into_the_sibling`、`resizing_a_split_changes_the_tree_and_the_panes` |
| Swift 纯逻辑 | `LayoutModelTests`（4 个）：divider 命中按轴且限定在它的 box 内、pane 命中、焦点循环回绕、`proposedRatio` 是纯坐标映射、空模型无焦点 |

## 2. GUI（真键盘 / 真鼠标，`tools/m1-gui-check.sh`）

脚本用 System Events 发快捷键、用 `cliclick` 发真实鼠标事件，窗口先被钉到 (0,0)/900×560。
真实输出（macOS）：

```text
panes before splitting: 1
after Cmd-D: 2 shells, 1 split_pane sent
after Cmd-Shift-D: 4 shells
focus messages: 1
resize_split messages: 2
shells after the window closed: 4
reopened window: 4 panes restored
screenshots in docs/images/m1: dragged.jpg reopen.jpg split-2.jpg split-3.jpg split-4.jpg
```

| 项 | 证据 |
|---|---|
| ⌘D 左右分屏 | ![split-2](images/m1/split-2.jpg) |
| 再来一次 → 三格 | ![split-3](images/m1/split-3.jpg) |
| ⌘⇧D → 四格 | ![split-4](images/m1/split-4.jpg) |
| 鼠标拖动第一个分隔线（450 → 600） | ![dragged](images/m1/dragged.jpg)，宿主收到 2 条 `resize_split` |
| 点击左侧 pane 切焦点 | 宿主日志 `focus messages: 1` |
| 关窗后 4 个 shell 仍在 → 重开 | ![reopen](images/m1/reopen.jpg)，`4 panes restored`，四个 `sh-3.2$` 都在 |

## 3. 这轮抓到的真 bug（都已修）

1. **协议缺 float 写入**：`SplitPane.ratio` / `ResizeSplit.ratio` 是 f32，但 C ABI 只有 u32/i32/bool/str/bytes 的 setter，
   客户端拼出来的消息被 schema 校验挡下（日志 `refusing to send an incomplete message`）。
   补了 `pg_msg_set_f32` / `pg_msg_get_f32`。
2. **客户端只 attach 第一个 pane**：重开窗口后另外三个 pane 是空白。改为收到 `layout` 就对尚未订阅的 pane 逐个 `attach`。
3. **split 不移动焦点**：新建的 pane 没成为焦点；宿主在 split 后显式 `layout.focus(new_pane)`。
4. **测试助手吃到过期 layout**：每次 attach 都会重播布局，测试改成「等到满足条件的 layout」再断言。

## 4. 未验 / 已知缺口

| 项 | 说明 |
|---|---|
| `zoom_pane` / `unzoom` | 协议按规范回 `unsupported`，UI 未做（见 `docs/plan.md` §11） |
| 分隔线拖动的“手感” | 只验证了拖拽会改变比例并重排，没有做最小宽度下的主观体验 |
| 侧栏 / SSH / IME | M3 / M4 / M6 |
| Ubuntu 实跑 | 由 CI 的 `rust (ubuntu-latest)` 覆盖（见下） |
