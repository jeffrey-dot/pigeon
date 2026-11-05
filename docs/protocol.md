# pigeon 协议（客户端 ⇄ 会话宿主）

> 这是**字段级契约**：`pigeon-server`（本机/远端）与任何客户端都照它实现。
> 另一条同源的边界是 C ABI（`include/pigeon_core.h`，由 `core/src/ffi/mod.rs` 生成），
> 用于“把内核编进客户端”的嵌入模式；两者共用同一份内核，字段语义必须一致（见 §9）。

## 0. 实现覆盖（2026-11-05 核对）

本文保留字段契约；定义了字段不等于所有路径已实现。当前差异如下：

- 宿主提供 `delta`、`scrollback`、`osc7501`、`status_all`、`status_snapshot`、`text_query`、`session_env`；macOS 请求 `delta`、`scrollback`、`status_all`、`status_snapshot`，Linux 请求 `delta`、`scrollback`、`status_all`、`status_snapshot`、`text_query`、`text_viewport`，首帧快照之后应用连续增量，回看由宿主 `scroll` 切片。
- `zstd` 是预留能力，宿主不提供，当前 payload 为裸 CBOR，无压缩 tag。
- `snapshot`/`delta` 已下发 OSC 8 链接，含历史、回看与续传；每帧独立解析为格子拥有的 URI，macOS 可 Cmd+click，Linux 可 Ctrl+click。设计/限制见 [OSC 8](osc8.md)。基础 cell 仍为单码点，附加码点通过 `text` 稀疏表传递。
- `new_session.env` 已应用到该会话及后续分屏，独立于宿主/其它会话；终端能力变量保留、输入上限及失败行为见 §3.2。结构化 `mouse` 仍返回 `unsupported`。
  macOS 将鼠标按当前 VT 模式编码成 `keys.bytes`，不是发送 `mouse`。
- `status_snapshot` 已实现完整未确认记录集合与独立修订号；macOS 已请求该能力，并通过 C ABI 读取；Linux 已通过安全 API 接入本地侧栏和逐条显式确认。
- `status_all` 通过独立 `status` 帧补发状态，协商 `status_snapshot` 后改用完整集合；`sessions` 本身不包含状态记录。未协商 `status_all` 的连接仍接收已 attach pane 的状态，不能靠未请求 `osc7501` 推断不会收到 `status`。
- 心跳当前由 macOS 客户端实现：15s 未收到宿主消息后 ping，45s 无消息重连；宿主响应 pong。
  Linux 握手后每 15s ping，45s 无完整合法帧时关闭连接并报告原因；已连接 socket 握手上限 5s，队列边界见 [Linux 传输](linux-transport.md)。本地自动恢复已接；同实例实时屏请求 attach.since，历史/搜索/新实例取全量，输入恢复与状态查看证明分别门控。
- 握手当前拒绝不同 major 并返回自身 `PROTO_VERSION`；“minor 取较小值”的目标尚未实现。
- `scroll` 及历史发送的宿主处理尚未完整按 `scrollback` 握手能力门控；官方客户端通过 `history:false` 避免 attach 传整段历史。
- 尺寸投票当前存在 `SessionState.vote`，同会话 pane 共用；尚未达到 §6 的 pane 级策略契约。首次投票者决定策略，仍有投票者时重新 attach 不会修改它。
- 宿主已实现 hello 5s / 入站空闲 45s 超时；版本拒绝或首帧非 hello 会发送答复后关闭连接。
- `close_pane` 目前调用 PTY child 的 `kill()`；尚无显式的 SIGHUP 等待后 SIGKILL 的两阶段策略。

这些差异是待完成项，不改变协议的兼容性规则；新增能力仍需同时补实现和测试。

## 1. 传输与分帧

| 项 | 规定 |
|---|---|
| 传输 | 本机 unix socket（`$XDG_RUNTIME_DIR/pigeon.sock`，否则 `/tmp/pigeon-<uid>/pigeon.sock`；socket `0700`，所在目录必须属于当前用户且组/其他人不可写，或是 root 所有且带 sticky 位；双方用 `lstat` 校验属主与类型，不跟随符号链接；宿主用对端凭据、客户端用 `getpeereid` 确认对方是同一 uid，否则拒绝）；远端为同一协议的 stdio（`ssh host pigeon-server --proxy`） |
| 帧 | `u32` 小端长度 `L` + 恰好 `L` 字节 payload。`L` **不含自身**；payload 为单个 CBOR map。不用换行分隔 |
| 上限 | 单帧 ≤ 8 MiB（8388608）。超限即断开并回 `error{code:"frame_too_large"}`，payload 不进解析器 |
| 半包 | 读端必须能处理“长度够但 payload 不足”的中间态，累积到 `L` 字节再解 |
| 心跳 | macOS 客户端 15s 无入站帧时发 `ping`，45s 无入站帧视为断线；宿主回 `pong`；Linux 每 15s ping，45s 完整帧静默断线，本地自动恢复已接 |
| 版本 | 握手期协商；**大版本不同即拒绝**，小版本向下兼容 |
| 压缩 | 只有双方 `features` 都含 `zstd` 时启用。启用后 payload 首字节是 tag：`0x00` = 原文，`0x01` = 其后为 zstd 帧。tag 计入 `L` |

## 2. 编码规则（CBOR，RFC 8949）

1. 每个 payload 是**定长 map**（`0xA_` 开头）。map 里必须有一个 `t: str`，值是消息名——各表「消息」列的字面量（如 `"snapshot"`）；其余 key 就是表里的字段名。
2. 请求标识用 `id: u32`（客户端生成，本连接内单调递增），**只有**表中列了 `id` 的消息带它。`id` 的唯一用途是**关联**：`created.id` 回显 `new_session.id`；任何请求失败时 `error.id` 回显该请求的 `id`。除 `created` 外，成功路径不带 `id`——靠消息语义表达（`attach` → `snapshot`；`close_pane`/`split_pane`/`resize_split`/`zoom_pane` → `layout` 广播）。
3. 整数一律用最短编码；只有标注 `i32` 的字段可以为负。`u32`/`u16`/`u8` 超范围即协议错误。
4. 浮点只有 `ratio`，用 float32（`0xFA`）。不传 float64。
5. 文本是 UTF-8 text string；唯一的 byte string 是 `keys.bytes`（`0x5_`）。
6. **不发 null**：字段缺失就是“没有”。收到 null 视为协议错误。
7. 未知 key 忽略；未知 `t` 回 `unsupported`（`t_of` = 那个 `t` 值；没有 `t` 或不是 str 时用空串），不断开。
8. 不要求确定性编码，接收方不得按字节比较两个等价 map。
9. 数组顺序有语义：`rows` 升序、`cells` 按列升序。

下文各表用 `name: type` 记字段，后缀 `?` 表示可选（可整个省略，但不可为 null）。

## 3. 消息

### 3.1 握手

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `hello` | `proto: u16`, `client: str`, `features: [str]` | — | 必须是第一帧；5s 内未收到则断开 |
| s→c | `hello_ack` | `proto: u16`, `server: str`, `features: [str]`, `host_id: str`, `client_id: u32` | `instance: str` | `client_id` 由宿主分配，本连接内唯一；`instance` 每次宿主进程启动都会变，`seq` 与 pane id 随之从头开始，客户端只能对同一个 `instance` 发 `attach.since` |
| s→c | `hello_error` | `reason: "version"\|"busy"\|"unauthorized"`, `detail: str` | — | 发完即关闭连接 |

- `proto = (major << 8) | minor`。当前 **major = 1，minor = 0**，即线上的 `0x0100`。
- 大版本不同 → `hello_error{reason:"version"}`；小版本以较小者为准，双方只能用 `features` 交集里的能力。

### 3.2 会话与 pane

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| s→c | `sessions` | `items: [{session_id: u32, title: str, cwd: str, panes: [u32]}]`（c→s 的请求里可以省略，视为空数组） | `active_pane: u32`, `active_title: str`（每个 item 内的可选字段） | 连接建立后的首次全量列表；标题/cwd 变化时宿主会重新下发整张列表；`active_pane` 是 layout 里的焦点，`active_title` 来自 OSC 0/2 |
| c→s | `new_session` | `id: u32` | `cwd: str`, `shell: str`, `env: {str: str}` | `env` 与宿主环境合并，同名覆盖；上限及保留变量见下文 |
| s→c | `created` | `id: u32`, `session_id: u32`, `pane_id: u32` | — | `id` 回显请求的 `id` |
| c→s | `close_pane` | `id: u32`, `pane_id: u32` | — | 关闭 pane 及其进程组（SIGHUP → SIGKILL） |
| c→s | `split_pane` | `id: u32`, `pane_id: u32`, `axis: "v"\|"h"`, `ratio: f32` | — | `v` = 左右并排，`h` = 上下堆叠；`ratio` ∈ (0,1)，被夹到最小尺寸内 |
| c→s | `resize_pane` | `pane_id: u32`, `cols: u16`, `rows: u16` | — | 是**请求**：是否生效由尺寸策略决定（§6），结果以 `size_state` 广播 |
| c→s | `focus` | `pane_id: u32` | — | 客户端把焦点告诉宿主；宿主记在布局树里，并随 `layout.focus` 广播，供重连复原 |
| c→s | `resize_split` | `id: u32`, `split_id: u32`, `ratio: f32` | — | 拖动分隔线；`split_id` 来自上一帧 `layout`；`ratio` 被夹到最小尺寸内 |
| c→s | `zoom_pane` | `id: u32`, `pane_id: u32` | — | 放大单个 pane（对应 `layout.zoom`） |
| c→s | `unzoom` | `id: u32`, `pane_id: u32` | — | 取消放大 |
| s→c | `layout` | `session_id: u32`, `tree: node` | `zoom: u32`, `focus: u32` | `node = {leaf: pane_id} \| {split: {id: u32, axis: "v"\|"h", ratio: f32, a: node, b: node}}`；`zoom` 是当前放大的 `pane_id`；`focus` 是宿主持有的焦点 pane（M1 新增，可选，旧客户端忽略） |
| s→c | `size_state` | `pane_id: u32`, `cols: u16`, `rows: u16`, `source: str`, `clients: u8` | — | `source` ∈ `latest`/`smallest`/`locked`；客户端据此在侧栏标注当前尺寸来源 |
| s→c | `pane_exited` | `pane_id: u32`, `code: i32` | — | 进程退出；pane 记录保留到 `close_pane`。`attach` 一个已经退出的 pane 时，快照后紧跟一条 |

#`new_session.env` 为该会话启动环境覆盖，后续 split 继承相同覆盖，不读取另一个 pane 运行中的进程环境。
省略或空 map 仅继承宿主环境；值为空字符串确实覆盖为空，不表示删除。宿主环境与其它会话不变。
名称为 `[A-Za-z_][A-Za-z0-9_]*`、最多 255 UTF-8 字节；值最多 32 KiB、不得包含 NUL；
最多 128 项，按 `Σ(name bytes + value bytes + 2)` 合计最多 64 KiB。宿主固定的 `TERM`、`COLORTERM`、
`PI_PROGRAM_STATUS` 不允许请求覆盖；任意非法项在创建 PTY 前整份拒绝，返回 `error{code:"invalid_arg",id}`，不回显变量值。
宿主继承环境和操作系统自身的 exec 大小限制仍可能导致创建失败。`session_env` 能力用于客户端探测旧宿主是否真的应用 env；
已定义的 env 字段不要求新能力才可发送，新客户端如依赖覆盖效果应先确认协商成功。当前 GUI 尚无环境编辑入口。

## 3.3 屏幕

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `attach` | `id: u32`, `pane_id: u32` | `size_policy: str`, `history: bool`, `since: u32` | `size_policy` ∈ `latest`(默认)/`smallest`/`locked`；`history: false` 时答复的 `snapshot` 不带 `history`（靠 `scroll` 回看的客户端用，省掉每次切会话最多 2 MB 的历史），缺省 = 按 `scrollback` 能力；`since` 见下方“断线续传” |
| s→c | `snapshot` | `pane_id: u32`, `seq: u32`, `cols: u16`, `rows: u16`, `alt: bool`, `cursor: {x: u16, y: u16, visible: bool, shape: u8}`, `scroll: {top: u16, bottom: u16}`, `screen: [[cell]]` | `history: [[cell]]`, `links: [{id: u32, uri: str}]` | 全量；`screen` 必须恰好 `rows` 行 × `cols` 列；`history` 仅在 `scrollback` 能力协商成功时出现；`links` 仅在至少一个 `cell.link` 非 0 时出现 |
| s→c | `delta` | `pane_id: u32`, `seq: u32`, `rows: [{y: u16, cells: [cell]}]` | `cursor: {x: u16, y: u16, visible: bool, shape: u8}`, `scroll: {top: u16, bottom: u16}`, `links: [{id: u32, uri: str}]`, `alt: bool` | 只含脏行；`alt` 报备用屏状态（仅收增量的客户端靠它知道 `vim` 起来了）；`cells` 长度必须等于 `cols`；`cursor`/`scroll` 的字段与 `snapshot` 同名同型 |
| c→s | `scroll` | `pane_id: u32`, `offset: i32` | — | 负 = 往历史看（`-1` = 上滚一行）；`0` 回到底部。宿主按**每连接**记住位置，回一帧该位置的 `snapshot`（不带 `history`）；回看期间该 pane 不再推实时帧，`offset: 0` 才恢复 |

- `snapshot` 与 `delta` 都带可选 `modes: u32`（`core/src/term` 的 `MODE_*` 位）：`1` 应用光标键（DECCKM `?1`）、
  `2` 括号粘贴（`?2004`）、`4` 鼠标点击上报（`?1000`）、`8` 拖动上报（`?1002`）、`16` 全部移动上报（`?1003`）、
  `32` SGR 鼠标格式（`?1006`）、`64` 焦点事件（`?1004`）、`128` 应用小键盘、`256` 备用屏滚轮转方向键（`?1007`）、
  `512` 光标闪烁（DECSCUSR 1/3/5）、`1024`–`16384` kitty 键盘协议当前生效的 flag
  （程序 `CSI > flags u` 栈顶，flag `1 << n` 对应位 `1024 << n`：消歧 / 事件类型 / 备选键 / 全部按键转义 / 附带文本）。
  客户端据此决定方向键、鼠标与滚轮发什么；缺省 = 0。

- `seq` 只由 `delta` 递增（每 pane 从 1 开始）；`snapshot` 带的是它已包含的最新 `delta` 的 `seq`，自己不占号。所以别的窗口 `attach`/`scroll` 不会在这个窗口的增量流里留下空洞。客户端收到 `seq <= last` 的增量直接丢（快照已经包含）；`seq > last + 1` → 丢掉增量、重新 `attach`。
- **断线续传**（issue #4）：重连的客户端在 `attach.since` 里带上该 pane 屏幕上已有的最后一个 `seq`。宿主每 pane 保留最近的
  `delta`（最多 4096 帧且合计 512 Ki 格），`since` 之后的帧都还在就按顺序只补这些帧（之后接实时流），**不发快照**；
  帧已被淘汰、中间 resize 过（缓冲清空）、`since` 比当前还大（另一个宿主进程给的号）或连接没协商 `delta` 时，按没带 `since` 处理，回一帧快照。
  客户端断线后保留屏幕不清空，重连后对已画过的 pane 带 `since`；`hello_ack.instance` 变了就丢掉所有 `seq`，改要快照。
- 宿主大量输出时按 16 ms 合并脏行发 `delta`；按键之后的首次输出优先立即 flush，不受该间隔限制；pane 尺寸变化后不发 `delta`，而是给每个观看者推一帧 `snapshot`（脏行装不下新的列数）。
- **慢客户端**：宿主给每个连接一条出站队列。某 pane 已有一帧 `delta` 在队里没发出去时又来一帧，两帧都换成该 pane 的一帧
  `snapshot`（发出时现取，即最新屏幕；`seq` 照常是它包含的最新 `delta`），所以读得慢的客户端看到的是 `delta … snapshot`
  而不是一长串 `delta`，`seq` 不会跳号。这帧 `snapshot` 不带 `history`。`layout`/`size_state`/`status`/`status_snapshot`/`pane_exited`/`sessions`
  与请求的答复从不丢弃、彼此保持顺序。队列里编码后的字节超过 64 MiB 时宿主直接断开该连接（不发 `backpressure`），客户端照
  §7 重连、重新 `attach`。
- `cursor.shape`：`0` 方块、`1` 下划线、`2` 竖线。
- `cell = [codepoint: u32, attrs: u8, fg: u32, bg: u32, link?: u32]`。
  - 宽字符由 `codepoint` 的 `wcwidth` 推导；宽字符占两格，第二格是空 continuation，客户端按宽度跳过绘制。
  - `attrs` 位：`1` 粗体、`2` 斜体、`4` 下划线、`8` 反色、`16` 删除线、`32` 暗色、`64` 软换行（行末格后接下一行，不插入文本换行）。一旦发布只加位、不改语义。
  - 颜色：`0x01000000 | idx` 表示调色板索引；否则按 `0xRRGGBB`。
  - `link` 缺省/0 = 无超链接；非 0 时只指向同一帧 `links` 表里的 `id`。接收时解析为 cell 自有 URI；
    delta 的未替换行保留自有 URI，不能借用后来帧的表。缺失表/未知编号降级为普通文字。
    新宿主编号在 pane 内单调分配、不复用；snapshot/scroll 带所显示 cells 的表，delta 带替换行的完整表。
    表最多 1024 项、URI 合计 256 KiB；URI 最多 2048 UTF-8 字节、13 个未转义分号，拒绝控制/空白/方向控制字符。
    编号 0、重复编号、非法 URI 或超限表拒绝；宿主导出超限时只省略链接、继续显示格子。
    原始 OSC 8 校验、所有权、预算与 http/https 打开策略见 [OSC 8](osc8.md)。
- `snapshot` / `delta` 可带 `text: [{row:i32, col:u16, text:str}]`：完整单元格文本（主码点 + 零宽附加码点），空表省略。
  `row >= 0` 表示屏幕；快照 `row < 0` 从历史末尾倒数（-1 最新），delta 的 row 必须是本帧被替换的行。
  未列出的格子仅有主码点；快照/行替换同时清除旧附加文本，不能把旧表保留到新行上。
  接收端拒绝越界、重复坐标、主码点不一致、控制字符以及单格 >4096 UTF-8 字节；旧消息不带表仍合法，旧客户端忽略表。
  导出极长附加序列在码点边界截到 4096 字节；历史预算与 backlog 计费包括附加文本。见 [ADR-0012](adr/0012-cell-text.md)。
- 同一 `pane_id` 的 `snapshot` 会把该客户端的 `seq` 基准重置。
- `scroll` 的答复是一个 `snapshot`，额外带两个只有它才有的字段：`history_len`（滚回缓冲一共多少行）与 `offset`（这一屏回看的行数，0 = 实时屏）。客户端据此画滚动条位置；别的 `snapshot` 不带这两个字段。
- `scroll` 的 offset 是**相对当前位置的增量**（除了 `0` 表示“回到底部”），宿主把它夹在 `[0, history_len]`，所以多按几次 PageUp 不会越界；窗口各自回看，互不影响。
- `history` 只在 `snapshot` 里出现（即时序上跟 `attach` 走）；增量帧不带历史。macOS 回看走 `scroll` 获取一屏，不靠重复 attach 拉历史。自行请求 `history` 的消费者可将它与同一帧 `screen` 拼接，但不得把旧历史与后来的实时屏幕当成连续内容。
- 宿主按单帧上限（8 MB）裁剪历史：超宽 pane 上只发最新的若干行，旧的丢掉而不是让 `attach` 失败。客户端不能假定「`history` 一定包含全部历史」。
- `layout.tree` 用 `{leaf: 0}` 表示「这个 session 已经没有 pane 了」（pane id 从 1 开始）。

### 3.4 输入与状态

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `appearance` | — | `fg: u32`, `bg: u32`, `ansi: [u32]`（16 个 `0xRRGGBB`） | 客户端当前主题，供宿主回答程序发出的 `OSC 4/10/11` 颜色查询。**缺省值**：客户端还没发过时，宿主用内置的深色板（底 `#0E0F11` / 字 `#E6E6E6` + harp 的 `DARK_MODE_*` 十六色）作答；`fg`/`bg` 缺失或 `ansi` 不是 16 个时，保留上一次的答复（不 panic、不猜） |
| c→s | `keys` | `pane_id: u32`, `bytes: bstr` | — | 已编码好的终端字节（macOS 支持 legacy/Kitty keyboard protocol，按 modes 协商编码；Linux 同样支持 legacy/Kitty keyboard protocol） |
| c→s | `mouse` | `pane_id: u32`, `kind: "down"\|"up"\|"move"\|"wheel"`, `x: u16`, `y: u16`, `button: u8`, `mods: u8` | — | `mods` 位：`1` shift、`2` alt、`4` ctrl |
| c→s | `paste` | `pane_id: u32`, `text: str` | — | 宿主负责括号粘贴包装：程序开了 `?2004` 才包 `ESC[200~ … ESC[201~`，并去掉正文里的起止标记（防止剪贴板内容提前结束粘贴、把后面当命令执行）；`\n`/`\r\n` 换成 `\r` |
| s→c | `status` | `pane_id: u32`, `state: "idle"\|"working"\|"blocked"\|"done"\|"error"`, `seq: u32` | `kind: "permission"\|"question"\|"auth"`, `msg: str`, `app: str`, `id: str`, `age_ms: u64` | 来自 OSC 7501；`msg`/`app` 只当纯文本；`app` 是上报的 agent 名（`app=`），`id` 是记录 id；`age_ms` = 进入当前 `state` 已多少毫秒（宿主时钟上的时长，不是时间戳，两端时钟不必一致；同一状态重复上报不重置） |
| s→c | `status_snapshot` | `pane_id: u32`, `revision: u64`, `records: [status_record]` | — | 仅协商同名能力后发送；完整未确认记录集合，原子替换该 pane 旧集合；空数组表示清空 |

`status_record = {id: str, seq: u32, state: str, kind: str, msg: str, app: str, age_ms: u64}`：
所有字段必选，文本缺省值为 `""`；state 是与旧 status 相同的五个值，kind 是 `""`/`permission`/`question`/`auth`。
`id: ""` 表示 pane 默认记录。宿主按 id 排列；接收端不得将数组顺序解释成 UI 优先级。

- **记录身份与集合修订号分离**：record seq 仅标识记录更新，用于 `ack_status`；删除或 ack 后，较低 seq
  仍可重新成为旧路径的当前记录。revision 从 0 开始，记录变更、成功 ack、实际删除均递增；重复/过期/live ack、
  无效 OSC、纯读取不递增。revision 与屏幕 seq 无关，仅同 host instance、pane 内可比较。
- 新能力的每一帧都是全量；未出现的记录明确从集合删除，原因可能是 ack、clear、提示符、退出或同 id 更新。
  **缺失不代表本客户端已查看**。空集合也必须应用。客户端在新连接首次全量建立基准，同连接忽略不大于已应用 revision
  的后续集合；instance 变化清空旧基准。允许 revision 跳号，不需要状态增量续传。
- 握手协商 `status_all` 时，在 sessions 后发送所有 pane 全量（包括空集合）；否则 attach 后发送该 pane 全量。
  attach.since 只影响屏幕，状态始终全量；事件总线丢帧恢复同样重发全量。ack 成功后立即广播，
  不等待下一次 PTY 输出；多个客户端观察同一个宿主集合，任一客户端确认都会使该记录从新路径删除。
- 每 pane 最多 256 条存储记录；容量满时只回收已确认记录，无可回收槽位则忽略新 id，上报已有 id 仍可更新。
  不因容量静默丢弃未确认终态。seq 到 u32 上限后拒绝新记录更新，ack/clear 仍可用，不复用身份。
- wire 校验拒绝 pane_id/record seq 为 0、非空集合 revision 为 0、重复 id/seq、非法 state/kind、
  id/msg/app 控制字符、单记录三个文本 UTF-8 长度之和 >4096、记录数 >256，以及必选字段缺失/null/类型错误。
  单帧仍受 8 MiB 限制。详见 [状态同步与双外形读取](status-sync.md)。

- `status` 的生命周期由内核负责：进程退出或 OSC 133A 提示符 → 丢弃 `working`/`blocked`；
  `done`/`error` 保留到客户端 `ack_status`。`ack_status` 对 `working`/`blocked` 无效：那是 agent 当下的状态，
  用户正看着也不能被“看过”清掉。旧路径在成功 ack 后也立即广播下一条未确认记录；为空时发 idle/seq=0。
- `msg` 含控制字符或解不出 UTF-8 → 整条丢弃（不带 `msg` 也不改写），见 `AGENTS.md` §7 不变量 3。

### 3.5 控制

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `ack_status` | `pane_id: u32`, `seq: u32` | — | 用户已看过该条状态，可被淘汰 |
| c→s | `ping` | `ts: u64` | — | `ts` = 发送方单调时钟（毫秒），只用于回显 |
| c→s | `shutdown` | — | — | 结束宿主：杀掉所有 pane 的进程、删掉 socket 后退出（窗口的 “End All Sessions on This Mac…”）；`hello` 之前发同样按 `protocol` 错误处理 |
| s→c | `pong` | `ts: u64` | — | 原样回显 `ping.ts` |
| s→c | `unsupported` | `t_of: str` | — | 收到不认识的消息；不断开 |
| s→c | `error` | `code: str`, `detail: str` | `id: u32` | `id` 回显出错请求 |

### 3.6 文本查询（2026-11-05 核对）

仅协商 `text_query` 后使用；不要求 attach、不改变尺寸或滚动视口。macOS 搜索另需协商 `text_viewport`，见 [搜索接入](search-progress.md)。

| 方向 | t | 字段 | 说明 |
|---|---|---|---|
| c→s | `text_query` | `id:u32`, `pane_id:u32`, `query:map` | 一页有界请求；query.kind 见下表 |
| s→c | `text_query_result` | `id:u32`, `pane_id:u32`, `result:map` | 复用请求 id；result.kind 与请求一致 |

| kind | query 字段 | result 字段 |
|---|---|---|
| `info` | 无 | `info` |
| `search` | `generation:u64`, `pattern:str`, `case_sensitive:bool`, `max_results:u16`, 可选 `start:text_point` | `info`, `hits:[text_range]`, 可选 `next:text_point` |
| `range` | `generation:u64`, `range:text_range` | `info`, `text:str`, 可选 `next:text_point` |
| `viewport` | `generation:u64`, `range:text_range` | `info`, `range:text_range`, `viewport:map`；须同时协商 `text_viewport` |

`info = {generation:u64, first_row:i32, last_row:i32, cols:u16, alt:bool}`；历史为负行，屏幕从 0 开始。
`text_point = {row:i32,col:u16,byte:u16}`；`text_range = {start:text_point,end:text_point}` 半开区间。
格内 byte 必须为 UTF-8 码点边界，宽字符续格无文本；行末 col=cols,byte=0。范围提取保留组合码点，合并软换行、保留 LF 硬换行、裁掉硬行末 ASCII 空格。

search 的 generation=0 且省略 start 表示新查询；续页传返回的 generation 和 next。
range 必须使用非零 generation；续页把 range.start 替换成 next，end 保持原值。
feed（包括 pending output）、resize、同步输出 flush 都使旧 generation 失效，返回 `stale_text` 后整次重查。
旧位置还必须与宿主 instance、pane_id 绑定，不能跨宿主重启复用。

`viewport` 必须带非零 generation。在 pane actor 的同一次串行操作内校验 generation 与两端坐标、
读取一屏；任何已 feed 的 pending output、resize、历史淘汰、备用屏切换都会使旧位置被拒绝。
`viewport = {rows:u16,cursor,scroll,screen,text?,links?,modes:u32,offset:u32,history_len:u32}`；
格子、稀疏文本和链接表沿用 snapshot 的编码/验证规则，没有 `history` 或实时 `seq`。
画面顶行对应 `-offset`，offset=`max(-range.start.row,0)`；info.cols/alt/last_row 与画面一致。
命中在实时屏时 offset=0，历史命中起点在画面顶行；跨屏范围只展示这一屏。
请求 id、pane_id、generation、range 和画面作为同一答复返回。客户端只能把该 range 高亮在这份画面上，
不能把旧 scroll 答复或后续实时 delta 混入。查询不修改连接的旧 scroll offset；关闭/输入返回实时屏仍发
`scroll{offset:0}`，以其普通 snapshot 恢复实时 seq 基准。
该结果是不可变的查询画面，持续输出后不能继续用于定位其它命中；重新定位/分页必须再次校验 generation。

literal 允许 LF、拒绝其它控制字符，长度 1–256 UTF-8 字节；大小写选项只折叠 ASCII，不归一化 Unicode。
每页最多 128 个命中、128 行/131072 格计费、64 KiB 文本，KMP 线性扫描；无法在预算内向前推进返回 `limit`。
无命中且有 next 仍需继续；省略 next 才表示完成。停止请求下一页即取消，服务器不保留搜索任务。
结构类型错误沿用协议错误路径；类型正确但模式/范围/数量非法由宿主返回带 id 的 `invalid_arg`，连接保持。
完整语义、接入流程及未完成项见 [文本查询](text-query.md)。

## 4. 错误码

| `code` | 含义 | 发完是否断开 |
|---|---|---|
| `version` | 大版本不兼容 | 是 |
| `busy` | 宿主已达会话/连接上限 | 是 |
| `unauthorized` | 非同一用户 / socket 权限不符 | 是 |
| `protocol` | 畸形帧、CBOR 解不出、字段类型不符、出现 null | 是 |
| `frame_too_large` | 单帧超 8 MiB | 是 |
| `shutdown` | 宿主正常退出 | 是 |
| `no_such_pane` | `pane_id` 不存在 | 否 |
| `stale_text` | 文本 generation 失效，丢弃旧范围/分页内容并重查 | 否 |
| `limit` | 有界文本查询无法推进或 generation 已耗尽 | 否 |
| `invalid_arg` | 文本模式、范围或结果数量非法 | 否 |
| `unsupported` | 未协商 text_query 却发送新请求；错误携带 id | 否 |
| `bad_request` | 字段组合非法（如 `ratio` 越界且无法夹取） | 否 |
| `backpressure` | 预留错误码；当前慢连接合并屏幕帧，超限直接断开（§3.3） | 若收到，客户端应重新 `attach` |
| `internal` | 宿主内部错误，已写日志 | 否 |

## 5. 能力（`features`）

| 名称 | 方向 | 语义 | 未协商时的行为 |
|---|---|---|---|
| `session_env` | c→s | 宿主应用会话 env 覆盖并由后续分屏继承；限制与保留变量见 §3.2 | 旧宿主可能忽略 env，依赖覆盖效果时应禁用入口 |
| `text_query` | 双向 | 有界 literal 搜索和范围提取；独立 generation、请求 id、分页 | 新客户端禁用入口；新宿主拒绝未协商请求 |
| `text_viewport` | 双向 | 与 `text_query` 一起允许 `kind:viewport`，同代次原子定位画面 | 保持 info/search/range；拒绝未协商的 viewport，错误携带 id |
| `delta` | s→c | 快照之后发 `delta` | 只发 `snapshot`；客户端按需重新 `attach` |
| `scrollback` | s→c | `snapshot.history` 与 `scroll` 可用 | 不下发历史，客户端隐藏滚动条 |
| `zstd` | 双向 | 帧 payload 带 1 字节 tag，`0x01` 为 zstd | payload 必须是不带 tag 的裸 CBOR |
| `osc7501` | s→c | 发 `status` | 不发 `status`，侧栏不显示 agent 状态 |
| `status_snapshot` | s→c | 完整状态集合 + 独立 revision；与 `status_all` 正交，协商后替代旧 `status`，不同时发送两种外形 | 保持旧 `status` 单记录路径 |
| `status_all` | s→c | 不 `attach` 也收到**所有** pane 的状态（按 `status_snapshot` 协商选外形）：握手后紧跟 `sessions` 补发每个 pane 当前未确认的状态，之后实时推送；未 attach 的状态观察者不进尺寸投票；同一连接之后 attach 会参与对应 pane 的投票 | 只收已 `attach` 的 pane 的 `status`（侧栏要靠逐个 `attach` 才能看到后台会话） |

新增能力只是往表里加一行；客户端必须忽略不认识的名称。

## 6. 多客户端尺寸策略

- 策略是**pane 级**的，由第一个 `attach` 的客户端决定，后续 `attach` 只做确认；改策略要重新 `attach`。
- `latest`（默认）：最近一次发 `focus`/`keys`/`paste` 的客户端说了算；当前 macOS 鼠标字节走 `keys`。
- `smallest`：取所有已 attach 客户端中 `cols`/`rows` 的最小值（tmux 的保守做法）。
- `locked`：只有 `size_policy` 首次设定时的客户端能改，其它客户端的 `resize_pane` 被忽略。
- 每次尺寸变化都广播 `size_state`，`source` 告诉客户端“现在是谁说了算”，`clients` 是观察者数量。
- 客户端必须在侧栏显示 `source`（`AGENTS.md` §7 不变量 6）。

## 7. 断开与重连

- 客户端断开不影响宿主；重连走一次新的 `hello + attach`。
- 宿主退出 → 所有连接收到 `error{code:"shutdown"}` 后关闭。
- 客户端发现 `seq` 跳号 / 收到 `backpressure` → 重新 `attach` 拿快照，不要尝试补齐。
- 读得太慢、出站队列超过上限（§3.3）的连接会被宿主断开；这与断线相同，客户端重连即可。

## 8. 兼容规则

1. 新增字段：必须可选，旧接收方忽略；不要复用已有字段名。
2. 新增消息：接收方不认识时回 `unsupported`，不回错误、不断开。
3. `cell` 的 `attrs` 与颜色编码一旦发布不改语义，只加位。
4. 快照的 `screen` 必须填满 `cols × rows`（缺失单元格由客户端按默认色填充）。
5. 只有追加字段才允许小版本内变更；改字段含义或类型必须 bump 大版本。

## 9. 与 C ABI 的关系

| | 嵌入模式（C ABI） | 进程模式（本协议） |
|---|---|---|
| 谁持有 PTY | 客户端进程 | `pigeon-server` |
| 数据形态 | 直接调用函数，返回快照指针 | 同一份快照，序列化成帧 |
| 适用 | 本机、追求最低延迟 | 持久化、远端、多客户端 |

字段映射（`include/pigeon_core.h` ↔ 本协议）：

| C ABI | 协议 |
|---|---|
| `PgSnapshot.screen`（行数组） | `snapshot.screen`（`rows` × `cols` 二维数组） |
| `PgSnapshot.cols` / `PgSnapshot.rows` | `snapshot.cols` / `snapshot.rows`（都是行/列**数**） |
| `PgSnapshot.alt_screen` | `snapshot.alt` |
| `PgSnapshot.scroll_region` | `snapshot.scroll` |
| `PgSnapshot.history` + `history_count` | `snapshot.history` |
| `PgCell.codepoint/attrs/fg/bg` | `cell[0..3]`（必定存在） |
| `PgCell.hyperlink_id` + `PgSnapshot/PgDelta.links` | `cell[4]` + 同帧 `links`；表由快照/delta 持有，PgCell 步长不变 |
| `PgCell.width` | 由 `codepoint` 推导，不上线 |
| `PgStatus.state/kind/msg/seq` | `status.state/kind/msg/seq`（旧 focused-pane 接口包含已确认存储记录） |
| `pg_msg_set_session_env` | 与 Rust `api::set_session_env` 共用有界校验、失败不修改消息；先设置 new_session.id，再逐项设置 env |
| `pg_text_query` / `pg_msg_text_query` / `pg_msg_text_result` + `pg_text_result_info/hit/text/free` | 同一 TextQuery 语义用于嵌入 Core 与宿主请求；结果 handle 独立拥有文本，Rust 为 `Core::text_query` / `text_query_message` / `text_result_of` |
| `pg_text_result_snapshot` / `pg_text_result_viewport` | viewport 结果的独立快照与 offset/history_len/modes；Rust 为 `text_viewport_snapshot` / `text_viewport_of`；kind=3，`pg_text_result_hit(0)` 读取同画面范围 |
| `pg_status_snapshot_new` / `pg_msg_status_snapshot` + `pg_status_snapshot_info/at/free` | `status_snapshot` 的完整未确认集合；Rust 为 `Core::status_snapshot` / `api::status_snapshot_of`，独立所有权、读取不消费 |
| `PgRect` | `layout.tree` 的展开结果，不上线 |
| `pg_pane_dirty_rows` | `delta.rows[].y` |

**语义必须一模一样**：`docs/core.md` 的 `Term` 接口是本协议字段的唯一实现来源，不允许两边各写一套。

## 10. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0 | 初版：握手、会话/pane、快照与增量、输入、状态、尺寸策略 |
