# pigeon 内核（core）设计

> 结论先行：**内核只写一次，用可移植语言实现并暴露客户端接口（C ABI + 与之一一对应的 Rust API）；各平台只写薄客户端。**
> 客户端可以每个平台各写一份（AppKit / GTK / 未来的 Windows），内核与后台会话进程共用同一份代码。
> 语言已选 **Rust**（理由见 §6），VT 引擎从 `alacritty_terminal` 起步，用我们自己的 `Term` trait 包住。

## 1. 为什么要单独设计内核

- 终端的行为正确性（VT 解析、光标、备用屏、宽字符、终端模式）是长期资产，**重复实现 = 重复踩坑**。
- 跨平台意味着客户端至少两份；如果内核也跟着平台走，就是两套语义、两套 bug。
- “任务活过窗口”需要一个后台进程；它与嵌入模式必须跑**同一份**内核，否则重连后的屏幕会漂移。

## 2. 边界：什么在内核里，什么不在

| 层 | 内容 | 平台相关？ |
|---|---|---|
| **core** | VT 状态机（parser + screen）、滚动缓冲、pane 的字节进出、快照/增量编码、OSC 7501 记录与生命周期、布局模型（树 + 比例 + 最小尺寸）、会话模型（会话/pane 关系；窗口属于客户端） | ❌ 纯逻辑，无 IO、无平台 API |
| **core 之外（后台进程侧）** | PTY（`portable-pty` 的 forkpty 封装）、信号、socket 服务、进程生命周期 | 只用 POSIX（macOS/Linux 都有） |
| **客户端** | 窗口、侧栏控件、终端绘制（字形/颜色/光标）、IME、剪贴板、鼠标光标、快捷键 | ✅ 每平台一份 |

判断标准：**能不能在没有窗口系统的进程里跑出完全一样的结果**。能 → 内核；不能 → 客户端。

## 3. 两种消费方式，同一份内核

1. **嵌入（复用方式）**：客户端把 core 编进进程（Swift/C 走静态库或 dylib + C ABI；Rust 客户端直接依赖 crate 并只用 `pigeon_core::api`，见 §4.1），
   由调用方驱动字节输入与快照消费。pigeon 自身的客户端不持有 PTY；macOS 的 C 档通过 `LocalSession` 调用嵌入内核解析普通 SSH 输出。
2. **会话宿主（持久化，默认）**：`pigeon-server` 用同一份 core 持有 PTY 与会话状态，窗口通过 socket 消费
   **快照 + 增量**。远端场景（ssh）也走这条。

两条路径共享：同一套 parser/screen、同一份快照格式、同一套 OSC 7501 记录规则。
**不允许**客户端自己实现第二份 VT 解析、布局计算或 OSC 7501 语义；字形光栅化和绘制本来就属于客户端。

## 4. C ABI（`include/pigeon_core.h`）

**唯一权威是那个头文件**，本节只解释为什么这么定；头文件由 `cbindgen` 从 `core/src/ffi/mod.rs` 生成，
**手改无效**（CI 会比对提交版本与生成结果）。改 ABI 就改 `core/src/ffi/`，然后重新生成。

函数分组（完整签名、结构体字段、所有权注释以头文件为准）：

| 组 | 函数 | 用途 |
|---|---|---|
| 版本与错误 | `pg_core_abi_version` `pg_last_error` `pg_last_error_code` | 能力探测、线程本地错误读取 |
| 生命周期 | `pg_core_new` `pg_core_free` | 会话容器（内含多 pane） |
| pane | `pg_pane_new` `pg_pane_free` `pg_pane_feed` `pg_pane_resize` `pg_pane_respond` | 字节进出；PTY 归宿主，内核不做 IO |
| 快照 | `pg_pane_snapshot` `pg_pane_dirty_rows` `pg_snapshot_free` | attach 全量 / 之后只发脏行 |
| 状态 | `pg_status_count` `pg_status_at` `pg_status_ack` | OSC 7501 记录的读取与“已看过”确认 |
| 布局 | `pg_layout_split` `pg_layout_split_of` `pg_layout_close` `pg_layout_resize` `pg_layout_rects` `pg_layout_focus` `pg_layout_focused` | 纯数据。`split` 返回**新 pane id**并把焦点移过去；`close` 让兄弟占满父矩形；`resize` 按最小尺寸夹取；`rects` 按树序返回归一化矩形（cap 不够报 `PG_ERR_OVERFLOW`）；`split_of` 给出该 pane 旁边的分隔线 id |
| 消息 | `pg_msg_new` `pg_msg_free` `pg_msg_type` `pg_msg_set_u32/i32/bool/str/bytes/str_array` `pg_msg_get_u32/i32/bool/str` `pg_msg_encoded_len` `pg_msg_encode` `pg_msg_decode`（payload）`pg_msg_encode_frame` `pg_msg_decode_frame`（带长度前缀）`pg_msg_snapshot` `pg_msg_sessions_count` `pg_msg_session_at` `pg_msg_session_panes` | 协议编解码：Swift 侧零 CBOR，字段名即 `docs/protocol.md` 的字段名 |

约定（同样写在头文件里，这里只说“为什么”）：
- 字符串是 **UTF-8 + 长度**（不带 NUL 终止）；**每个结构体**（含 `PgCell` / `PgCursor` / `PgScrollRegion`）
  首字段是 `size`：调用方可以传更大的 `size`，内核忽略尾部未知字节；传小了报 `PG_ERR_INVALID_ARG`。
  每 cell 的新数据优先加 `attrs` 位而不是加字段（与 `docs/protocol.md` 的 `cell` 编码规则一致）。
- 内核**不持有**任何平台句柄（无 fd、无窗口、无字体）；PTY 读到的字节由宿主喂进来，应答字节由宿主写出去。
- 所有调用**同步**、非阻塞（`pg_pane_feed` 只解析，不做 IO）；不存在回调，宿主自己轮询脏行/状态。
- 同一个 `PgCore` 上的调用由调用方串行化（一次一个线程）；`PgSnapshot` 不可变，可跨线程。
- 出错返回 `NULL`/0/`false` + `pg_last_error()`，不抛异常、不 abort（panic 必须在 FFI 边界被捕获，报 `PG_ERR_PANIC`）。
- ABI 变更规则：只能**追加**字段（靠 `size` 判定）；要改已有字段含义或顺序，就 bump `PG_CORE_ABI_VERSION` 并让客户端拒绝加载。

### 4.1 Rust 客户端接口（`pigeon_core::api`）

C ABI 是给非 Rust 客户端的。Rust 客户端（例如 M5 的 GTK4 客户端用 `gtk4-rs`）如果也走 C 头，
就要用 `unsafe` 裸指针调同一门语言的代码，手动释放 `PgSnapshot`，丢掉 Rust 选型的主要收益；
模块私有性则通过 `internals` feature 控制。因此客户端接口定成**一份语义、两种外形**：

- `core/src/api/`：安全的 Rust API，分组与 §4 的表一一对应（pane / 快照 / 状态 / 布局 / 消息），
  用所有权表达生命周期，错误是 `Result<T, PgError>`。
- `core/src/ffi/` 的强制规则是只通过 `api` 做薄包装，负责指针/长度转换、错误码和 panic 捕获。`api::delta_of` 与 `pg_msg_delta` 已共用转换/校验，FFI 不再直接 import 内部模块；`tools/checks/ffi-imports.sh` 约束依赖方向。
- `term` / `layout` / `status` / `session` / `proto` 只在 `internals` feature 下是 `pub`（默认开，给 `pigeon-server`
  与 `core/tests/` 用）；客户端以 `default-features = false` 依赖时它们是私有模块，`api` 只重导出客户端需要的类型
  （快照/格子、状态记录、布局矩形、协议 `Message` 与编解码），“客户端不认识内部类型”在编译期成立。
- Rust 客户端是独立 crate（`clients/linux/`），不进 `core/`。

用例：`core/tests/api.rs` 测 Rust 外形，`core/tests/abi.rs` 测 C 外形。

## 5. 快照与增量

```text
PgSnapshot {            // 字段以 include/pigeon_core.h 为准，此处只写形状
  size, version, pane_id, cols, rows, alt_screen,
  cursor: { size, x, y, visible, shape },
  scroll_region: { size, top, bottom },
  screen: [ PgRow { size, cells: [ PgCell { size, codepoint, fg, bg, attrs, hyperlink_id, width } ] } ],
  history: [ PgRow ], history_count   // 只在 attach 时需要（可选，控制内存）
}
```
- **attach 时**：通常回全量快照；同一宿主 `instance` 内带 `since` 且增量仍在 backlog 时，只补缺少的增量（见 [protocol.md §3.3](protocol.md#33-屏幕)）。
- **之后**：已协商 `delta` 的客户端接收脏行、光标与模式；未协商者接收快照。macOS 已消费 `pg_msg_delta` / `pg_delta_free`，Linux 已通过 `api::delta_of` 消费增量，缺序重新 attach。resize 时推全量快照。
- 快照是**不可变**的，宿主可以跨线程持有；内核不回收已被快照引用的内存，直到 `pg_snapshot_free`。
- **滚动缓冲**：嵌入接口 `PgConfig.scrollback_lines = 0` 取默认 10,000 行，上限 100,000；宿主当前固定 10,000 行，未暴露配置。macOS attach 使用 `history:false`，回看由宿主按 `scroll` 请求切出一屏。
- 软换行由 `ATTR_WRAP` / `PG_ATTR_WRAP`（64）标在行末格，快照/增量/历史保留；macOS 复制自动折行时不插入换行。
- **文本扩展**：Rust `Cell.text()` 返回主码点及附加文本；C ABI 在 PgSnapshot/PgDelta 尾部追加 PgCellText 表，表和字符串由快照持有并随其释放，PgCell 数组步长不变。
- **当前数据限制**：已保留引擎 cell 中的附加码点，但跨多个宽 cell 的完整字素宽度/整簇重排仍未实现；OSC 8 链接未下发，DECSTBM 的导出范围为整屏。详见 [plan.md §11](plan.md#11-当前能力与已知缺口)。
- **文本所有权与预算**：FFI 先保留文本/行缓冲的 Rust 所有权，完成所有可失败校验后再转交 C。历史与 backlog 计费包含文本；密集整屏仍可能超过单帧上限，限制与回归见 [text-followup.md](text-followup.md)。

## 6. 语言选型

| 选项 | 优点 | 代价 | 结论 |
|---|---|---|---|
| **Rust + `alacritty_terminal`** | 生态最全（`vte` 解析器 + 成熟 `Term` 状态机）；C ABI 天然；性能上限高；`musl` 静态链接能把 `pigeon-server` 压进 3–8MB | 多一门语言 + 多一套构建 + FFI 胶水；M0 变慢 | **推荐（已选）** |
| **Swift + C ABI（备选）** | 与客户端同语言：无 FFI 胶水、无第二套构建；SwiftTerm 的 `Terminal` 已被大量使用，且能在 Linux 编译；`@_cdecl` 导出 C ABI 给其它语言客户端 | 性能调优空间比 Rust 小；`musl` 静态链接不可用，远端单文件体积难控；被第三方重度复用时生态弱 | 备选（触发条件见 `docs/plan.md` §3.1） |
| C/C++ 核心 | 像 kitty 一样成熟的路子；C ABI 天然 | 手写内存管理，VT 状态机易出错 | 备选 |
| Zig（libghostty-vt） | 直接复用 Ghostty 的内核 | 依赖上游演进；生态较新 | 备选 |

**决定**：内核用 **Rust**，`alacritty_terminal` / `vte` 包在我们自己的 `Term` trait 后面
（`feed/resize/snapshot/scroll`），C ABI 头文件由 `cbindgen` 从 `core/src/ffi/mod.rs` 生成（见 §4）。
换成 Swift（SwiftTerm headless）或 libghostty-vt 时，只替换 `Term` 的实现，头文件、客户端、协议都不动。
当前 `tools/vt-parity` 对照已提交的 tmux 文本 golden；SwiftTerm 可作实现参考，尚未接入自动对账。换引擎只有在保留现有语义时才能不改接口；新增字素、图片等数据仍需扩展快照和协议。

## 7. 参考（读什么、偷什么）

| 项目 | 读哪部分 |
|---|---|
| `alacritty_terminal` / `vte` (Rust) | `Term` 状态机、`Handler` trait、脏行/尺寸处理——**依赖** |
| `wezterm-term` (Rust) | 同上，另一个成熟实现；对比两者对 OSC/DCS 的处理 |
| `libvterm` (C) / `pyte` (Python) | 对账用的参考实现（golden test） |
| `libghostty-vt` (Zig) | 内核/UI 分层的接口设计 |
| kitty `screen.c` / `parser.c` | 性能技巧与边缘 case 清单（宽字符、组合字符、备用屏） |
| tmux `input.c` | 控制序列的分类学（哪些要应答、哪些要吞掉）——注意：**我们要转发的比它多** |

## 8. 测试策略与实施状态

1. **已接入文本对账**：`tools/vt-parity/run.sh` 的 8 条语料与 tmux golden 逐行比较；属性由确定性单测部分覆盖。完整属性对账、其它引擎与 `vttest` 语料仍待补。
2. **模糊测试目标**：随机/畸形字节流不得 panic、不得越界；当前尚无 `cargo fuzz` 工程或 CI 任务，畸形输入主要靠确定性测试。
3. **快照一致性**：`feed 全量 → snapshot` 与 `feed 全量 → 断开 → 重放快照` 输出必须逐 cell 相同。
4. **OSC 7501 规范表**：规范里的每条丢弃规则一个用例（超长、解不出 base64、含控制字符、`clear` 子树、256 条上限）。
5. **性能基准**：已有回看切片测试与 macOS `tools/bench/render-bench.sh`；16 pane、弱网、端到端输入延迟与功耗基线尚未完整建立。

## 9. 与里程碑的对应

| 里程碑 | 内核相关 |
|---|---|
| M0 | 内核库 + 最小 C 头 + macOS 客户端 + 本地 `pigeon-server`（关窗口不断任务） |
| M1 | 布局在核心里（`pg_layout_*`），客户端只画矩形与分隔线 |
| M2 | 协议版本协商、多窗口同看一个会话、后台进程崩溃提示 |
| M3 | 状态记录（OSC 7501）从核心里读；侧栏只做展示与排序 |
| M5 | Linux 客户端（GTK）复用同一内核与协议，不重写语义；Rust 写就经 `pigeon_core::api`（§4.1） |
