# pigeon 协议（客户端 ⇄ 会话宿主）

> 这是**字段级契约**：`pigeon-host`（本机/远端）与任何客户端都照它实现。
> 另一条同源的边界是 C ABI（`include/pigeon_core.h`，由 `core/src/ffi/mod.rs` 生成），
> 用于“把内核编进客户端”的嵌入模式；两者共用同一份内核，字段语义必须一致（见 §9）。

## 1. 传输与分帧

| 项 | 规定 |
|---|---|
| 传输 | 本机 unix socket（`/tmp/pigeon-<uid>.sock`，`0700`）；远端为同一协议的 stdio（`ssh host pigeon-host --proxy`） |
| 帧 | `u32` 小端长度 `L` + 恰好 `L` 字节 payload。`L` **不含自身**；payload 为单个 CBOR map。不用换行分隔 |
| 上限 | 单帧 ≤ 8 MiB（8388608）。超限即断开并回 `error{code:"frame_too_large"}`，payload 不进解析器 |
| 半包 | 读端必须能处理“长度够但 payload 不足”的中间态，累积到 `L` 字节再解 |
| 心跳 | 任一方 15s 无出站帧则发 `ping`；45s 无任何入站帧视为断线 |
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
| c→s | `new_session` | `id: u32` | `cwd: str`, `shell: str`, `env: {str: str}` | `env` 与宿主环境合并，同名覆盖 |
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
| s→c | `pane_exited` | `pane_id: u32`, `code: i32` | — | 进程退出；pane 记录保留到 `close_pane` |

### 3.3 屏幕

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `attach` | `id: u32`, `pane_id: u32` | `size_policy: str`, `history: bool`, `since: u32` | `size_policy` ∈ `latest`(默认)/`smallest`/`locked`；`history: false` 时答复的 `snapshot` 不带 `history`（靠 `scroll` 回看的客户端用，省掉每次切会话最多 2 MB 的历史），缺省 = 按 `scrollback` 能力；`since` 见下方“断线续传” |
| s→c | `snapshot` | `pane_id: u32`, `seq: u32`, `cols: u16`, `rows: u16`, `alt: bool`, `cursor: {x: u16, y: u16, visible: bool, shape: u8}`, `scroll: {top: u16, bottom: u16}`, `screen: [[cell]]` | `history: [[cell]]`, `links: [{id: u32, uri: str}]` | 全量；`screen` 必须恰好 `rows` 行 × `cols` 列；`history` 仅在 `scrollback` 能力协商成功时出现；`links` 仅在至少一个 `cell.link` 非 0 时出现 |
| s→c | `delta` | `pane_id: u32`, `seq: u32`, `rows: [{y: u16, cells: [cell]}]` | `cursor: {x: u16, y: u16, visible: bool, shape: u8}`, `scroll: {top: u16, bottom: u16}`, `links: [{id: u32, uri: str}]`, `alt: bool` | 只含脏行；`alt` 报备用屏状态（仅收增量的客户端靠它知道 `vim` 起来了）；`cells` 长度必须等于 `cols`；`cursor`/`scroll` 的字段与 `snapshot` 同名同型 |
| c→s | `scroll` | `pane_id: u32`, `offset: i32` | — | 负 = 往历史看（`-1` = 上滚一行）；`0` 回到底部。宿主按**每连接**记住位置，回一帧该位置的 `snapshot`（不带 `history`）；回看期间该 pane 不再推实时帧，`offset: 0` 才恢复 |

- `snapshot` 与 `delta` 都带可选 `modes: u32`（`core/src/term` 的 `MODE_*` 位）：`1` 应用光标键（DECCKM `?1`）、
  `2` 括号粘贴（`?2004`）、`4` 鼠标点击上报（`?1000`）、`8` 拖动上报（`?1002`）、`16` 全部移动上报（`?1003`）、
  `32` SGR 鼠标格式（`?1006`）、`64` 焦点事件（`?1004`）、`128` 应用小键盘、`256` 备用屏滚轮转方向键（`?1007`）。
  客户端据此决定方向键、鼠标与滚轮发什么；缺省 = 0。

- `seq` 只由 `delta` 递增（每 pane 从 1 开始）；`snapshot` 带的是它已包含的最新 `delta` 的 `seq`，自己不占号。所以别的窗口 `attach`/`scroll` 不会在这个窗口的增量流里留下空洞。客户端收到 `seq <= last` 的增量直接丢（快照已经包含）；`seq > last + 1` → 丢掉增量、重新 `attach`。
- **断线续传**（issue #4）：重连的客户端在 `attach.since` 里带上该 pane 屏幕上已有的最后一个 `seq`。宿主每 pane 保留最近的
  `delta`（最多 4096 帧且合计 512 Ki 格），`since` 之后的帧都还在就按顺序只补这些帧（之后接实时流），**不发快照**；
  帧已被淘汰、中间 resize 过（缓冲清空）、`since` 比当前还大（另一个宿主进程给的号）或连接没协商 `delta` 时，按没带 `since` 处理，回一帧快照。
  客户端断线后保留屏幕不清空，重连后对已画过的 pane 带 `since`；`hello_ack.instance` 变了就丢掉所有 `seq`，改要快照。
- 宿主每 pane 每 16 ms 最多发一帧 `delta`（大量输出时合并脏行）；pane 尺寸变化后不发 `delta`，而是给每个观看者推一帧 `snapshot`（脏行装不下新的列数）。
- `cursor.shape`：`0` 方块、`1` 下划线、`2` 竖线。
- `cell = [codepoint: u32, attrs: u8, fg: u32, bg: u32, link?: u32]`。
  - 宽字符由 `codepoint` 的 `wcwidth` 推导；宽字符占两格，第二格是空 continuation，客户端按宽度跳过绘制。
  - `attrs` 位：`1` 粗体、`2` 斜体、`4` 下划线、`8` 反色、`16` 删除线、`32` 暗色。一旦发布只加位、不改语义。
  - 颜色：`0x01000000 | idx` 表示调色板索引；否则按 `0xRRGGBB`。
  - `link` 缺省 = 无超链接；非 0 时指向同一帧 `links` 表里的 `id`（`links` 随该帧的 `snapshot`/`delta` 下发）。
- 同一 `pane_id` 的 `snapshot` 会把该客户端的 `seq` 基准重置。
- `scroll` 的答复是一个 `snapshot`，额外带两个只有它才有的字段：`history_len`（滚回缓冲一共多少行）与 `offset`（这一屏回看的行数，0 = 实时屏）。客户端据此画滚动条位置；别的 `snapshot` 不带这两个字段。
- `scroll` 的 offset 是**相对当前位置的增量**（除了 `0` 表示“回到底部”），宿主把它夹在 `[0, history_len]`，所以多按几次 PageUp 不会越界；窗口各自回看，互不影响。
- `history` 只在 `snapshot` 里出现（即时序上跟 `attach` 走）；增量帧不带历史。客户端要回看时重新 `attach` 拿最新历史——快照里的 `history` 与同一帧的 `screen` 是连续的，拼起来没有缝。
- 宿主按单帧上限（8 MB）裁剪历史：超宽 pane 上只发最新的若干行，旧的丢掉而不是让 `attach` 失败。客户端不能假定「`history` 一定包含全部历史」。
- `layout.tree` 用 `{leaf: 0}` 表示「这个 session 已经没有 pane 了」（pane id 从 1 开始）。

### 3.4 输入与状态

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `appearance` | — | `fg: u32`, `bg: u32`, `ansi: [u32]`（16 个 `0xRRGGBB`） | 客户端当前主题，供宿主回答程序发出的 `OSC 4/10/11` 颜色查询。**缺省值**：客户端还没发过时，宿主用内置的 harp 深色板（`#050505` / `#ffffff` + `DARK_MODE_*` 十六色）作答；`fg`/`bg` 缺失或 `ansi` 不是 16 个时，保留上一次的答复（不 panic、不猜） |
| c→s | `keys` | `pane_id: u32`, `bytes: bstr` | — | 已编码好的终端字节（kitty protocol 或 legacy 由客户端决定） |
| c→s | `mouse` | `pane_id: u32`, `kind: "down"\|"up"\|"move"\|"wheel"`, `x: u16`, `y: u16`, `button: u8`, `mods: u8` | — | `mods` 位：`1` shift、`2` alt、`4` ctrl |
| c→s | `paste` | `pane_id: u32`, `text: str` | — | 宿主负责括号粘贴包装：程序开了 `?2004` 才包 `ESC[200~ … ESC[201~`，并去掉正文里的起止标记（防止剪贴板内容提前结束粘贴、把后面当命令执行）；`\n`/`\r\n` 换成 `\r` |
| s→c | `status` | `pane_id: u32`, `state: "idle"\|"working"\|"blocked"\|"done"\|"error"`, `seq: u32` | `kind: "permission"\|"question"\|"auth"`, `msg: str`, `app: str`, `id: str`, `age_ms: u64` | 来自 OSC 7501；`msg`/`app` 只当纯文本；`app` 是上报的 agent 名（`app=`），`id` 是记录 id；`age_ms` = 进入当前 `state` 已多少毫秒（宿主时钟上的时长，不是时间戳，两端时钟不必一致；同一状态重复上报不重置） |

- `status` 的生命周期由内核负责：进程退出或 OSC 133A 提示符 → 丢弃 `working`/`blocked`；
  `done`/`error` 保留到客户端 `ack_status`。`ack_status` 对 `working`/`blocked` 无效：那是 agent 当下的状态，
  用户正看着也不能被“看过”清掉。
- `msg` 含控制字符或解不出 UTF-8 → 整条丢弃（不带 `msg` 也不改写），见 `AGENTS.md` §7 不变量 3。

### 3.5 控制

| 方向 | 消息 | 必选字段 | 可选字段 | 说明 |
|---|---|---|---|---|
| c→s | `ack_status` | `pane_id: u32`, `seq: u32` | — | 用户已看过该条状态，可被淘汰 |
| c→s | `ping` | `ts: u64` | — | `ts` = 发送方单调时钟（毫秒），只用于回显 |
| s→c | `pong` | `ts: u64` | — | 原样回显 `ping.ts` |
| s→c | `unsupported` | `t_of: str` | — | 收到不认识的消息；不断开 |
| s→c | `error` | `code: str`, `detail: str` | `id: u32` | `id` 回显出错请求 |

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
| `bad_request` | 字段组合非法（如 `ratio` 越界且无法夹取） | 否 |
| `backpressure` | 客户端读得太慢，宿主丢弃了 `delta` | 否（客户端应重新 `attach`） |
| `internal` | 宿主内部错误，已写日志 | 否 |

## 5. 能力（`features`）

| 名称 | 方向 | 语义 | 未协商时的行为 |
|---|---|---|---|
| `delta` | s→c | 快照之后发 `delta` | 只发 `snapshot`；客户端按需重新 `attach` |
| `scrollback` | s→c | `snapshot.history` 与 `scroll` 可用 | 不下发历史，客户端隐藏滚动条 |
| `zstd` | 双向 | 帧 payload 带 1 字节 tag，`0x01` 为 zstd | payload 必须是不带 tag 的裸 CBOR |
| `osc7501` | s→c | 发 `status` | 不发 `status`，侧栏不显示 agent 状态 |
| `status_all` | s→c | 不 `attach` 也收到**所有** pane 的 `status`：握手后紧跟 `sessions` 补发每个 pane 当前未确认的状态，之后实时推送；这类连接不进任何会话的尺寸投票 | 只收已 `attach` 的 pane 的 `status`（侧栏要靠逐个 `attach` 才能看到后台会话） |

新增能力只是往表里加一行；客户端必须忽略不认识的名称。

## 6. 多客户端尺寸策略

- 策略是**pane 级**的，由第一个 `attach` 的客户端决定，后续 `attach` 只做确认；改策略要重新 `attach`。
- `latest`（默认）：最近一次发 `focus`/`keys`/`mouse` 的客户端说了算。
- `smallest`：取所有已 attach 客户端中 `cols`/`rows` 的最小值（tmux 的保守做法）。
- `locked`：只有 `size_policy` 首次设定时的客户端能改，其它客户端的 `resize_pane` 被忽略。
- 每次尺寸变化都广播 `size_state`，`source` 告诉客户端“现在是谁说了算”，`clients` 是观察者数量。
- 客户端必须在侧栏显示 `source`（`AGENTS.md` §7 不变量 6）。

## 7. 断开与重连

- 客户端断开不影响宿主；重连走一次新的 `hello + attach`。
- 宿主退出 → 所有连接收到 `error{code:"shutdown"}` 后关闭。
- 客户端发现 `seq` 跳号 / 收到 `backpressure` → 重新 `attach` 拿快照，不要尝试补齐。

## 8. 兼容规则

1. 新增字段：必须可选，旧接收方忽略；不要复用已有字段名。
2. 新增消息：接收方不认识时回 `unsupported`，不回错误、不断开。
3. `cell` 的 `attrs` 与颜色编码一旦发布不改语义，只加位。
4. 快照的 `screen` 必须填满 `cols × rows`（缺失单元格由客户端按默认色填充）。
5. 只有追加字段才允许小版本内变更；改字段含义或类型必须 bump 大版本。

## 9. 与 C ABI 的关系

| | 嵌入模式（C ABI） | 进程模式（本协议） |
|---|---|---|
| 谁持有 PTY | 客户端进程 | `pigeon-host` |
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
| `PgCell.hyperlink_id` | `cell[4]`（可选；缺省 = 无链接） |
| `PgCell.width` | 由 `codepoint` 推导，不上线 |
| `PgStatus.state/kind/msg/seq` | `status.state/kind/msg/seq` |
| `PgRect` | `layout.tree` 的展开结果，不上线 |
| `pg_pane_dirty_rows` | `delta.rows[].y` |

**语义必须一模一样**：`docs/core.md` 的 `Term` 接口是本协议字段的唯一实现来源，不允许两边各写一套。

## 10. 版本历史

| 版本 | 变更 |
|---|---|
| 1.0 | 初版：握手、会话/pane、快照与增量、输入、状态、尺寸策略 |
