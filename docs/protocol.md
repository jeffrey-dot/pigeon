# pigeon 协议（客户端 ⇄ 会话宿主）

> 这份是**字段级契约**：`pigeon-host`（本机/远端）与任何客户端都照它实现。
> 另有一张同源的 C ABI（`include/pigeon_core.h`）用于“把内核编进客户端”的嵌入模式，两者共用同一份内核。

## 1. 传输与分帧

| 项 | 规定 |
|---|---|
| 传输 | 本机 unix socket（`/tmp/pigeon-<uid>.sock`，权限 0700）；远端为同一协议的 stdio（`ssh host pigeon-host --proxy`） |
| 分帧 | **长度前缀**：`u32` 小端长度 + payload（不含自身）；payload 为 CBOR（二进制，字段名短）。不用换行分隔 |
| 上限 | 单帧 ≤ 8 MiB；超限即断开并记录（防止恶意/异常客户端） |
| 心跳 | `ping` / `pong`，15s；45s 无响应视为断线 |
| 版本 | 握手期协商；**大版本不同即拒绝**，小版本向下兼容（未知字段忽略、未知消息忽略并回 `unsupported`） |

## 2. 消息

所有消息为 `{t: <type>, ...}`。`id` 只在需要应答的请求里出现（客户端生成，单调递增）。

### 2.1 握手

| 方向 | 消息 | 字段 |
|---|---|---|
| c→s | `hello` | `proto: u16`, `client: str`, `features: [str]` |
| s→c | `hello_ack` | `proto: u16`, `server: str`, `features: [str]`, `host_id: str` |
| s→c | `hello_error` | `reason: "version" \| "busy" \| "unauthorized"`, `detail: str` |

`features` 示例：`delta`, `scrollback`, `zstd`, `osc7501`。客户端只能用双方都有的能力。

### 2.2 会话与 pane

| 方向 | 消息 | 字段 |
|---|---|---|
| s→c | `sessions` | `items: [{session_id, title, cwd, panes: [pane_id]}]` |
| c→s | `new_session` | `id`, `cwd?, shell?, env?` |
| s→c | `created` | `id`, `session_id`, `pane_id` |
| c→s | `close_pane` | `id`, `pane_id` |
| c→s | `split_pane` | `id`, `pane_id`, `axis: "v" \| "h"`, `ratio: f32` |
| c→s | `resize_pane` | `pane_id`, `cols: u16`, `rows: u16` |
| c→s | `focus` | `pane_id` |
| s→c | `layout` | `session_id`, `tree: {leaf: pane_id} \| {split: {axis, ratio, a, b}}`, `zoom?: pane_id` |

### 2.3 屏幕

| 方向 | 消息 | 字段 |
|---|---|---|
| c→s | `attach` | `id`, `pane_id` |
| s→c | `snapshot` | `pane_id`, `seq`, `cols`, `rows`, `alt: bool`, `cursor: {x,y,visible,shape}`, `scroll: {top,bottom}`, `rows: [[cell]]` ※ |
| s→c | `delta` | `pane_id`, `seq`, `rows: [{y, cells: [cell]}], cursor?, scroll?` |
| c→s | `scroll` | `pane_id`, `offset: i32`（负=往历史看） |

※ `cell = [codepoint: u32, flags: u8, fg: u32, bg: u32]`；`flags` 位：1 宽字符延续，2 粗体，4 斜体，8 下划线，16 反色，32 删除线。颜色：`0x01000000 | idx` 表示调色板索引，否则为 `0xRRGGBB`。

`seq` 单调递增；客户端发现 `seq` 跳号（丢了 delta）→ 重新发 `attach` 拿快照。

### 2.4 输入与状态

| 方向 | 消息 | 字段 |
|---|---|---|
| c→s | `keys` | `pane_id`, `bytes:bstr` |
| c→s | `mouse` | `pane_id`, `kind: "down"\|"up"\|"move"\|"wheel"`, `x`, `y`, `button`, `mods` |
| c→s | `paste` | `pane_id`, `text: str` |
| s→c | `status` | `pane_id`, `state: idle\|working\|blocked\|done\|error`, `kind?: permission\|question\|auth`, `msg?: str`, `seq` |
| s→c | `pane_exited` | `pane_id`, `code: i32` |

`status` 来自 OSC 7501；内核负责记录的生命周期（进程退出/OSC 133A → 丢弃 `working`/`blocked`；`done`/`error` 保留到客户端 `ack_status`）。`msg` 只当纯文本，含控制字符的整条丢弃。

### 2.5 控制

| 方向 | 消息 | 字段 |
|---|---|---|
| c→s | `ack_status` | `pane_id`, `seq`（表示用户已看过） |
| c→s | `ping` / s→c | `pong` | `ts: u64` |
| s→c | `unsupported` | `t_of: str`（客户端能力不支持该消息） |
| s→c | `error` | `id?`, `code: str`, `detail: str` |

## 3. 错误与断开

- 协议错误（畸形帧、超限、未知大版本）→ `error` 后断开，写日志。
- 客户端断开不影响宿主；重连走一次新的 `hello + attach`。
- 宿主退出 → 所有连接收到 `error{code:"shutdown"}` 后关闭。

## 4. 兼容规则

1. 新增字段：必须可选，旧接收方忽略；不要复用已有字段名。
2. 新增消息：接收方不认识时回 `unsupported`，不回错误、不断开。
3. `cell` 的 `flags` 与颜色编码一旦发布不改语义，只加位。
4. 快照的 `rows` 必须填满 `cols × rows`（缺失单元格由客户端按默认色填充）。

## 5. 与 C ABI 的关系

| | 嵌入模式（C ABI） | 进程模式（本协议） |
|---|---|---|
| 谁持有 PTY | 客户端进程 | `pigeon-host` |
| 数据形态 | 直接调用函数，返回快照指针 | 同一份快照，序列化成帧 |
| 适用 | 本机、追求最低延迟 | 持久化、远端、多客户端 |

**语义必须一模一样**：`docs/core.md` 的 `Term` 接口是本协议字段的唯一实现来源，不允许两边各写一套。
