# 文本查询纵向切片

> 2026-11-05 设计及实现说明。S3 的核心/宿主查询与原子定位接口；macOS 搜索/同画面高亮接入见 [搜索进展](search-progress.md)，AppKit/GUI 待验；完整选择尚未接入。

## 坐标与生命周期

文本查询使用独立的 `generation:u64`，初始化为 1。每次非空 feed、resize、同步输出 flush 都使其递增，包含尚未发送 delta 的输出、备用屏切换和历史淘汰。达到整数上限后拒绝查询，绝不复用 generation。客户端将位置与 `(host instance, pane_id, generation)` 一起保存；generation 不符返回 `stale_text`，必须重新查询。generation 不是屏幕 seq，也不是稳定行标识；持续输出可能使分页反复失效。

`TextPoint {row:i32, col:u16, byte:u16}`：屏幕行从 0 开始，历史从负数到 -1；列为核心格子列；byte 为格子完整 UTF-8 文本内的码点边界偏移。范围是 `[start,end)`。宽字符只在主格提供文本，续格和行末为宽字符预留的占位格不产生文本。组合码点保留，命中可处于一个格子内部；客户端高亮应覆盖对应格子，不能把 byte 当列。行尾 `col=cols, byte=0` 表示硬换行位置或最后一行 EOF；软换行不产生换行字符。硬行末的 ASCII 空格裁掉，制表符按核心已有空格语义输出，不恢复原始 PTY 字节。

## 有界查询

- literal 按 UTF-8 码点串精确匹配，不归一化。`case_sensitive=false` 只折叠 ASCII A–Z，Unicode 其余码点保持精确。允许 LF 搜索硬换行；拒绝其它控制字符、空模式及超过 256 UTF-8 字节的模式。
- 每页最多 128 个命中，上限由调用方显式给出；返回顺序从最旧历史到屏幕，允许重叠命中。
- 单次最多读取 128 行、131072 个格子计费单位、64 KiB 文本。行尾测量按整行列数计费，格子文本按现有 4096 字节边界导出；单格超长附加码点沿用现有导出截断，不承诺引擎内部全部码点。搜索用线性 KMP，分页仅重扫最多一个模式长度的尾部，保留跨页/跨软换行命中。
- 返回 `next` 时调用方以同 generation 和 next 继续；没有命中但有 next 不是搜索完成。停止发送下一页就是取消，没有后台搜索任务或服务器游标资源。旧请求返回后客户端按请求 id 丢弃已取消结果。
- 范围提取同样分页，每页 text 和 next；不把全历史送 UI。每页请求重新校验 generation，不能把不同 generation 的部分静默拼起来。

## 协议与接口

沿用 [ADR-0006](adr/0006-length-prefixed-cbor-protocol.md) 的 CBOR 和协议 1.0 可选扩展，不改变旧字段。握手显式协商 `text_query`；未协商时新请求返回带 id 的 `unsupported` error。旧宿主没有该能力，客户端禁用该入口，不发送未知请求。

`text_query {id,pane_id,query}` / `text_query_result {id,pane_id,result}`。query 的 `kind` 为 `info`、`search`、`range` 或 `viewport`；result kind 与请求对应。`viewport` 额外需要协商 `text_viewport`。错误使用现有 `error {id,code,detail}`，含 `invalid_arg`、`stale_text`、`limit`、`no_such_pane`。

Rust API 与 C ABI 共用语义，见 [ADR-0003](adr/0003-one-semantics-two-shapes.md)。新 C 请求/结果元数据结构带 size/version，范围结构带 size；新增 opaque 结果 handle 管理独立拥有的结果和借用字符串。未来客户端通过 typed builder/结果访问器使用协议，无需自己编码 CBOR。

## 明确未完成

regex、Unicode case folding、稳定行锚点、反向扫描、跨历史拖选/自动滚动/矩形选择及 Linux 搜索 UI 未完成。macOS 已接上下命中、分页、取消及高亮，AppKit 类型检查和真实 GUI 由 macOS 验证，不能把 Linux Foundation 测试当成端到端验收。

`TextQuery::Viewport {generation,range}` / C kind=3 在同一次串行 pane 查询内校验代次和范围后生成一屏画面。
结果 `TextResult::Viewport {info,range,viewport}` 不包含全历史；历史起点位于顶行，实时命中保持实时屏几何。
`text_viewport_snapshot` / `pg_text_result_snapshot` 返回独立拥有的 snapshot；
`text_viewport_of` / `pg_text_result_viewport` 提供同画面的 modes、offset 和 history_len。
`pg_text_result_hit(0)` 取返回范围，快照用 `pg_snapshot_free` 释放。
此读取不改宿主连接 scroll offset、不消费 damage、不改变屏幕 seq；位置不能传给无 generation 的旧 scroll 作为定位。
客户端必须冻结结果画面，收到新输出后标记过期；只有新的定位答复才能为另一命中画高亮。

## 客户端接入顺序

1. hello.features 加 `text_query`，检查 hello_ack.features；没有该项就保持入口不可用。
2. 搜索发 `TextQuery::Search { generation:0, start:None, pattern, case_sensitive, max_results }`。Rust 用 `api::text_query_message(id,pane,query)`；C 初始化 `PgTextQuery.size/version`，kind=1，然后 `pg_msg_text_query` + 既有 `pg_msg_encode_frame`。
3. 按 request id 丢弃旧输入对应的答复；Rust `api::text_result_of`，C `pg_msg_text_result` 后 `pg_text_result_info/hit`。坐标保存 host instance、pane 和 generation。
4. 有 next 就带同 generation 和 next 请求下一页；新模式或取消后不要再请求旧页。范围 kind=2 必须带非零 generation，结果 text 为纯文本。
5. `stale_text` 后丢弃已累计页并重查；`limit` 明示缩小范围/模式；空 hits 且无 next 才是无命中。C 结果以 `pg_text_result_free` 释放，借用字符串随 handle 失效；消息用 `pg_msg_free` 释放，栈上请求结构不需要释放。

查询只适用于持有实时引擎的 pane。`PaneState::from_snapshot` 的冻结重建不拥有完整历史，也缺少引擎的行末宽字符占位标记，因此返回 Unsupported，不把快照误当完整文本来源。

## 验证记录（2026-11-05）

本机 Arch Linux x86_64、Rust stable。没有修改客户端代码，没有新增依赖。

- `cargo test --manifest-path core/Cargo.toml`：297 passed、2 ignored；新增的 PTY 淘汰回归加入后单独复跑完整 host suite，60 passed。默认总用例为 298 passed，两个 ignored 是既有基准/文档示例。
- `cargo test --manifest-path core/Cargo.toml --no-default-features`：202 passed、1 ignored；`--lib --no-default-features` 构建通过。
- `cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings` 及加 `--no-default-features` 均通过；`cargo fmt --manifest-path core/Cargo.toml --check` 通过。
- 新增 14 个 Rust 文本查询用例、3 个 C ABI 用例、3 个真实 PTY 用例。覆盖历史、硬软换行、宽字符行末占位/续格、组合码点、ASCII 大小写、无命中、畸形坐标/UTF-8/消息、pending output、resize、备用屏、淘汰、结果和扫描预算、分页中止；100,000 行及密集组合码点的预算回归通过。
- `tools/vt-parity/run.sh`：8 条 golden 全通过；`tools/checks/ffi-imports.sh` 通过。
- cbindgen 0.27 重新生成头文件，再生成到临时路径 diff 一致；C 编译/运行新接口 smoke test 通过。
- GNU release 宿主 1,961,832 bytes；`CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=rust-lld cargo build --release --manifest-path core/Cargo.toml --bin pigeon-server --target x86_64-unknown-linux-musl` 产物 2,080,200 bytes，`file` 确认为 static-pie，`--version` 可运行。均小于 8 MiB。musl 构建会提示忽略不支持的 cdylib，宿主二进制成功生成。
- 文档相对链接与 `git diff --check` 通过。

macOS/Ubuntu CI、Swift 构建、两端 GUI、其它发布架构、长期负载和搜索 p95 延迟尚未验证；有界工作量回归不替代真实性能基准。静态 musl 产物只做构建与启动 smoke，真实 PTY suite 使用本机 GNU 构建。

2026-11-05 主线程与 OSC 8 整合后的复验：默认 core 314 passed / 2 ignored，无默认 211 passed / 1 ignored；
GTK 42 passed / 2 ignored。TAB 行尾范围提取和跨硬换行搜索回归先失败，修复后查询专题 15/15；
附着组合码点的 tab 仍保留完整文本。行尾测量和格子导出共用 `ExportedText` 空格判定。
宿主阻塞读写装配独立到 `pane/pty_io.rs`，有界队列与 actor 的串行 `PaneState` 所有权不变。
