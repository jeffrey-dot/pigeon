# OSC 8 超链接

2026-11-05 实现核对。核心、宿主帧、Rust/C 客户端接口与 macOS 交互已接入；Linux 客户端尚未接点击。
本次 Linux 验证覆盖核心与 Swift Foundation/ABI 逻辑，AppKit 构建、hover 外观和真实 Cmd+click 仍须 macOS CI/GUI 验收。

## 输入与身份

`core/src/term/` 是唯一处理 OSC 8 的位置。接受 `ESC ] 8 ; params ; URI ST`，ST 为 `ESC \\` 或 BEL；
空 URI 关闭链接。上游 Alacritty 负责格子附着、覆盖、擦除、滚动、resize 重排和备用屏。
在 VTE 前的 `Osc8Filter` 只校验原始 OSC 8，不解释屏幕；这是因为 VTE 会静默去掉部分 C0 控制字节、修补无效 UTF-8。
分段输入与终止符跨 feed 仍按同一规则处理。非法、取消或超限的 OSC 8 关闭之前的链接，文字照常显示。

- URI 必须是有效 UTF-8，非空时最多 **2048 字节**，拒绝控制字符、空白与双向覆盖/隔离控制符。
- OSC 8 的参数与 URI 合计缓存最多 **3072 字节**。溢出后丢弃直到终止，不持续增长缓冲。
- URI 最多 **13 个未转义分号**，以适应当前 VTE 的 16 项 OSC 参数限制；更多分号须写为 `%3B`。
  不把被上游截断的 URI 当作另一条有效链接。
- 程序提供的 `id=` 不作为可信数字身份。每次有效开启分配 pane 内单调递增 u32，RIS/备用屏切换不重置计数；
  耗尽时后续链接降级为普通文字，绝不复用旧编号。相同 URI 的多次开启可有不同编号，不承诺 `id=` 的分组高亮语义。

## 所有权与同步

核心 `Cell.hyperlink: Option<Arc<str>>` 与 `Cell.link` 一起保留链接。快照不可变，关闭 pane/core 后也可继续读取。
每次导出用局部表共享字符串，无永久增长的 URI 注册表；上游 cell 的链接随网格/历史淘汰释放。

wire 复用协议 1.0 已有的 `cell[4]` 和 `links: [{id, uri}]`。表只描述**这一帧实际携带的格子**：

- snapshot：当前屏幕与随帧携带的历史；scroll 答复独立包含所显示历史窗口的链接。
- delta：只含本帧替换行的链接；resume backlog 保留完整消息，包括该表。
- 客户端收到帧时立即把编号解析成自有 URI。未替换行保留原 URI，替换行同时替换或清除链接。
  不维护一个跨帧全局编号表，也不把未知旧编号映射到新 URI。
- 缺失表或未知编号按无链接处理；编号 0、重复编号、无效 URI、表超限则拒绝该帧。
- resize 的既有续传屏障仍要求全量快照；重连/背压全量恢复不依赖旧链接表。

每次快照/delta 导出最多 **1024 条、256 KiB URI 字节**。超过额度的格子继续保留文字和样式，但没有链接；
后续导出可在额度允许时重新带链接。表随帧释放，客户端只保留当前格子的 URI。
历史 2 MiB 预检预算保守计入每个格子的 URI（共享 URI 也重复计费）；backlog 同样计入表和字符串成本。
这些是导出/队列预算，不是整个进程 RSS 上限：引擎仍按有限网格和滚动缓冲持有链接；附加文本整屏的既有
8 MiB 帧限制见 [文本预算](text-followup.md)。

## 两种客户端接口与兼容

- Rust：`api::Core::snapshot`、`api::snapshot_of`、`api::delta_of` 的 cell 直接拥有 URI；
  `api::cell_links` 返回这些 cells 对应的自有表。
- C：`PgSnapshot` / `PgDelta` 尾部追加 `link_count` / `links`；`PgLink` 含 size、id、UTF-8 URI。
  URI 借用到所属 snapshot/delta 释放为止，与原 `PgMsg`/`PgCore` 生命周期无关。
- **PgCell 步长保持 28 字节**，既有字段偏移不变，ABI version 仍为 1；旧客户端忽略新增尾字段继续显示文字。
  新客户端读取旧 wire 不带 `links` 时显示普通文字。新 macOS 客户端与随应用打包的新 core 链接，
  不承诺把新结构体直接用于旧动态库；跨进程旧宿主兼容由 wire 默认字段保证。
- `include/pigeon_core.h` 由 cbindgen 0.27.0 生成，不手改。

## macOS 交互与打开策略

`TerminalCell.hyperlink` 复制为 Swift String；snapshot/delta 的临时 C 表释放后仍可用。
悬停显示纯文本 `Command-click to open: …`；按住 Command 时鼠标为手形。
只有 Command 按下并在同一格、同一 URL 上抬起才调用 `NSWorkspace.open`；拖动或其间链接改变取消打开。
普通点击仍按选择/程序鼠标上报路径工作，链接手势不会向 PTY 发送不配对的鼠标 release。

首版只允许 **http/https**。必须有 host，拒绝用户名/密码、非法端口、反斜杠、控制/空白/方向控制字符、
无效百分号编码与百分号编码的控制字符。路径中的 `%20` 可用。`file`、`ssh`、`javascript`、`data`、`mailto`
及其它 scheme 均不打开；内核可保留合法 UTF-8 的其它 scheme 元数据，打开策略只在客户端执行。
没有 shell 拼接、URL 自动识别、自动打开、下载器或执行入口；显式打开后的网页行为由用户浏览器处理。

## 验证

- `core/tests/osc8.rs`：真实 OSC 8 字节、分段/控制字节/无效 UTF-8/过长/分号超限、未知与重复编号、
  覆盖/擦除、宽字符续格、换行、历史、resize、备用屏、RIS、快照/delta 编解码、scroll、resume 和恢复；资源超限不丢文字。
- `core/tests/osc8_budget.rs`：历史预检与 backlog 对 URI 字节计费，不让字符串绕过预算。
- `tools/checks/osc8-abi.sh`：C 编译器消费生成头文件，并按旧 cell/snapshot 前缀读取新库返回的对象。
- `core/tests/abi/osc8.rs`：PgCell 步长、拥有表、释放 core/message 后读取 snapshot/delta、Rust 对应表。
- `core/tests/host/osc8.rs`：真实 PTY 输出链接后断开、重新 attach，链接仍可读取。
- `TerminalLinksTests` / `TerminalLinksWireTests`：URL 白名单/恶意 URI、格子替换与实际 C ABI 字符串复制生命周期；
  可在 Linux 用 `tools/checks/swift-logic.sh` 与 `tools/checks/swift-status-wire.sh` 运行。
- 本地执行默认/无默认 feature 测试及 clippy、VT 8 条 golden、生成头文件一致性和 release 体积检查。
  完整结果随提交交付；macOS `swift build && swift test`、真实 hover/Cmd+click、Linux 客户端点击均未在这里验收。

2026-11-05 本 worktree 实测（Linux x86_64，Rust stable / Swift 6.1.2 临时工具链）：

| 检查 | 结果 |
|---|---|
| `cargo test -p pigeon-core` | 292 passed，2 ignored（含 58 项真实宿主测试） |
| `cargo test -p pigeon-core --no-default-features` | 194 passed，1 ignored |
| 默认/无默认 `cargo clippy --all-targets -- -D warnings` | 均通过 |
| `cargo build --lib --no-default-features` / fmt / FFI import 检查 | 均通过 |
| `tools/vt-parity/run.sh` | 8/8 golden 相符 |
| `tools/checks/osc8-abi.sh` / cbindgen 0.27.0 重生成 diff | 通过 / 无差异 |
| `tools/checks/swift-logic.sh` | 100 tests，0 failures |
| `tools/checks/swift-status-wire.sh`（含 TerminalLinksWireTests） | 3 tests，0 failures |
| `cargo build --release --bin pigeon-server` | 1,852,936 字节，低于 8 MiB；本机 GNU/Linux 构建，不作为 musl 静态产物验收 |
| 文档相对链接 / `git diff --check` | 均通过 |

AppKit 文件完成语法解析；此环境没有 macOS SDK，不能代替 AppKit 类型检查、完整 Swift 构建或 GUI 实测。

2026-11-05 主线程整合复审：CAN/SUB 在 `ESC ] 8` 前缀处取消的回归先失败，修复后分块与整段均通过；
modifier hover 更新并入既有 Kitty `flagsChanged`，避免重复 override。点击状态在每次应用画面时校验，
目标变更后即便恢复原 URL 仍保持取消；拖动、格子消失同样取消。链接表的 wire 与直接 API 路径共用校验。
整合后默认 core 293 passed / 2 ignored，无默认 195 passed / 1 ignored，Foundation 102 tests，
两种配置 clippy 与 VT 8/8 通过。AppKit 类型检查和真实交互等待整合提交的 macOS 验收。
