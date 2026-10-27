# pigeon 内核（core）设计

> 结论先行：**内核只写一次，用可移植语言实现并暴露 C ABI；各平台只写薄客户端。**
> 客户端可以每个平台各写一份（AppKit / GTK / 未来的 Windows），内核与后台会话进程共用同一份代码。
> 语言推荐 **Rust**（理由见 §6），VT 引擎从 `alacritty_terminal` 起步，用我们自己的 `Term` trait 包住。

## 1. 为什么要单独设计内核

- 终端的行为正确性（VT 解析、光标、备用屏、宽字符、鼠标、IME）是长期资产，**重复实现 = 重复踩坑**。
- 跨平台意味着客户端至少两份；如果内核也跟着平台走，就是两套语义、两套 bug。
- “任务活过窗口”需要一个后台进程；它与嵌入模式必须跑**同一份**内核，否则重连后的屏幕会漂移。

## 2. 边界：什么在内核里，什么不在

| 层 | 内容 | 平台相关？ |
|---|---|---|
| **core** | VT 状态机（parser + screen）、滚动缓冲、pane 的字节进出、快照/增量编码、OSC 7501 记录与生命周期、布局模型（树 + 比例 + 最小尺寸）、会话模型（session/window/pane 关系） | ❌ 纯逻辑，无 IO、无平台 API |
| **core 之外（后台进程侧）** | PTY（`forkpty`）、信号、socket 服务、进程生命周期 | 只用 POSIX（macOS/Linux 都有） |
| **客户端** | 窗口、侧栏控件、终端绘制（字形/颜色/光标）、IME、剪贴板、鼠标光标、快捷键 | ✅ 每平台一份 |

判断标准：**能不能在没有窗口系统的进程里跑出完全一样的结果**。能 → 内核；不能 → 客户端。

## 3. 两种消费方式，同一份内核

1. **嵌入（首选，低延迟）**：客户端把 core 编进进程（静态库或 dylib + C ABI），
   自己驱动渲染循环；PTY 可以由客户端进程持有，也可以接后台进程。
2. **会话宿主（持久化，默认）**：`pigeon-host` 用同一份 core 持有 PTY 与会话状态，窗口通过 socket 消费
   **快照 + 增量**。远端场景（ssh）也走这条。

两条路径共享：同一套 parser/screen、同一份快照格式、同一套 OSC 7501 记录规则。
**不允许**出现“客户端自己实现一份简化渲染逻辑”。

## 4. C ABI 草案（`include/pigeon_core.h`）

```c
// 句柄与生命周期
typedef struct PgCore PgCore;              // 一个会话容器（可包含多个 pane）
PgCore* pg_core_new(const PgConfig* cfg);
void    pg_core_free(PgCore*);

// pane（标准输入输出都是字节）
uint32_t pg_pane_new(PgCore*, const PgPaneOptions*);
void     pg_pane_free(PgCore*, uint32_t pane_id);
void     pg_pane_feed(PgCore*, uint32_t pane_id, const uint8_t* data, size_t len);
void     pg_pane_resize(PgCore*, uint32_t pane_id, uint16_t cols, uint16_t rows);
size_t   pg_pane_respond(PgCore*, uint32_t pane_id, uint8_t* out, size_t cap); // 终端查询应答

// 渲染：整屏快照 + 自上次以来的脏行
typedef struct { /* 见 §5 */ } PgSnapshot;
PgSnapshot* pg_pane_snapshot(PgCore*, uint32_t pane_id);   // 调用方负责 pg_snapshot_free
size_t      pg_pane_dirty_rows(PgCore*, uint32_t pane_id, uint16_t* rows, size_t cap);
void        pg_snapshot_free(PgSnapshot*);

// 状态（OSC 7501 记录）
size_t pg_status_count(PgCore*);
bool   pg_status_at(PgCore*, size_t index, PgStatus* out);  // pane_id, state, kind, msg(UTF-8), seq

// 布局（纯数据；view 由客户端实现）
uint32_t pg_layout_split(PgCore*, uint32_t pane_id, PgAxis axis, float ratio);
bool     pg_layout_close(PgCore*, uint32_t pane_id);
bool     pg_layout_resize(PgCore*, uint32_t split_id, float ratio);
size_t   pg_layout_rects(PgCore*, PgRect* out, size_t cap);  // pane_id + 归一化矩形
```

约定：
- 所有字符串是 **UTF-8 + 长度**（不带 NUL 终止）；所有结构体带 `size` 或 `version` 字段，便于加字段。
- 内核**不持有**任何平台句柄（无 fd、无窗口、无字体）；PTY 读到的字节由宿主喂进来，应答字节由宿主写出去。
- 所有调用**同步**、非阻塞（`pg_pane_feed` 只解析，不做 IO）；回调不存在，宿主自己轮询脏行/状态。
- 出错返回 `NULL`/0 + `pg_last_error()`，不抛异常、不 abort（panic 必须在 FFI 边界被捕获）。

## 5. 快照与增量

```text
PgSnapshot {
  version, pane_id, cols, rows,
  cursor: {x, y, visible, shape},
  alt_screen: bool,
  scroll_region: {top, bottom},
  rows: [ PgRow { cells: [ PgCell { codepoint, width, fg, bg, attrs, hyperlink_id } ] } ],
  scrollback: { first_line, count }   // 只在 attach 时需要（可选，控制内存）
}
```
- **attach 时**：全量快照（这是“重连后屏幕一致”的唯一保证，等价于 tmux 的 `capture-pane`）。
- **之后**：只发脏行（`pg_pane_dirty_rows`），脏行号由内核维护；光标/滚动区变化单独发。
- 快照是**不可变**的，宿主可以跨线程持有；内核不回收已被快照引用的内存，直到 `pg_snapshot_free`。

## 6. 语言选型

| 选项 | 优点 | 代价 | 结论 |
|---|---|---|---|
| **Swift + C ABI（推荐）** | 与客户端同语言：无 FFI 胶水、无第二套构建；SwiftTerm 的 `Terminal` 已被大量使用，且能在 Linux 编译；`@_cdecl` 导出 C ABI 给其它语言客户端 | 性能调优空间比 C/Rust 小；被第三方重度复用时生态弱 | **推荐（首发）** |
| **Rust + `alacritty_terminal`** | 生态最全；C ABI 天然；性能上限高 | 多一门语言 + 多一套构建 + FFI 胶水；M0 变慢 | 备选（触发条件见 `docs/plan.md` §3.1） |
| C/C++ 核心 | 像 kitty 一样成熟的路子；C ABI 天然 | 手写内存管理，VT 状态机易出错 | 备选 |
| Zig（libghostty-vt） | 直接复用 Ghostty 的内核 | 依赖上游演进；生态较新 | 备选 |

**决定**：内核用 **Swift + SwiftTerm 的 headless 核心**，用我们自己的 `Term` 协议包住
（`feed/resize/snapshot/scroll`），并通过 `@_cdecl` 导出 §4 的 C ABI。
换成 Rust（`alacritty_terminal`）或 libghostty-vt 时，只替换 `Term` 的实现，C ABI、客户端、协议都不动。
SwiftTerm 同时保留为**对账参考**（`tools/vt-parity`）。

## 7. 参考（读什么、偷什么）

| 项目 | 读哪部分 |
|---|---|
| `alacritty_terminal` / `vte` (Rust) | `Term` 状态机、`Handler` trait、脏行/尺寸处理——**首选依赖** |
| `wezterm-term` (Rust) | 同上，另一个成熟实现；对比两者对 OSC/DCS 的处理 |
| `libvterm` (C) / `pyte` (Python) | 对账用的参考实现（golden test） |
| `libghostty-vt` (Zig) | 内核/UI 分层的接口设计 |
| kitty `screen.c` / `parser.c` | 性能技巧与边缘 case 清单（宽字符、组合字符、备用屏） |
| tmux `input.c` | 控制序列的分类学（哪些要应答、哪些要吞掉）——注意：**我们要转发的比它多** |

## 8. 测试（内核是纯逻辑，所以能测得很硬）

1. **对账测试**：同一段字节流分别喂我们的内核与参考实现（`libvterm`/`pyte`/`tmux capture-pane`），
   逐行 + 逐属性比较；不一致就是 bug。语料：vim/htop/less/fzf/Pi TUI 的录制输出 + `vttest`。
2. **模糊测试**：随机/畸形字节流不得 panic、不得越界（Rust + `cargo fuzz`）。
3. **快照一致性**：`feed 全量 → snapshot` 与 `feed 全量 → 断开 → 重放快照` 输出必须逐 cell 相同。
4. **OSC 7501 规范表**：规范里的每条丢弃规则一个用例（超长、解不出 base64、含控制字符、`clear` 子树、256 条上限）。
5. **性能基准**：单 pane 1MB/s 输出时的 CPU 与帧时间；16 pane 场景。

## 9. 与里程碑的对应

| 里程碑 | 内核相关 |
|---|---|
| M0 | 内核库 + 最小 C 头 + macOS 客户端 + 本地 `pigeon-host`（关窗口不断任务） |
| M1 | 布局在核心里（`pg_layout_*`），客户端只画矩形与分隔线 |
| M2 | 协议版本协商、多窗口同看一个会话、后台进程崩溃提示 |
| M3 | 状态记录（OSC 7501）从核心里读；侧栏只做展示与排序 |
| M5 | Linux 客户端（GTK）复用同一内核与协议，不重写语义 |
