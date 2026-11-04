# 同 pane 多记录状态同步

实现核对日期：2026-11-05；基线 `cc51ce22aec38519ef038b6ac7f2d8791c36ccc2`。
本片覆盖内核、宿主和双外形客户端接口；客户端侧栏整合由后续任务完成。

## 设计

- 生命周期仍唯一由 `core/status` 持有；`seq: u32` 标识某次记录更新，
  `revision: u64` 标识一个 pane 的记录集合变更，两者与屏幕 seq 独立。
  ack、OSC 133A、进程退出、clear 都能改变 revision 而不产生新记录 seq。
- 新能力 `status_snapshot` 协商成功后，宿主用同名消息发送完整的**未确认记录集合**，
  包括 idle/working/blocked/done/error 的 id、seq、state、kind、msg、app、age_ms。
  每条消息原子替换该 pane 的集合；缺失的记录表示删除，空数组表示整 pane 清空。
  不额外引入变化补丁或删除日志，因此无补丁缺口、墓碑保留期或重连追赶依赖。
- `status_all` 决定观察范围，`status_snapshot` 决定消息外形，两者正交。
  未协商新能力时保持旧 `status` 消息；ack 成功后两种路径都立即广播剩余状态。
  握手后的全 pane 同步、attach（含屏幕续传）、事件总线丢帧恢复均重发完整集合。
- revision 只在同 host instance、pane 内比较；同一连接收过较新 revision 后忽略旧/重复集合。
  新连接首次全量必须建立基准，instance 变化则清除旧基准。age_ms 是采样时长，读取不增加 revision。
  数组顺序按 id，仅供稳定枚举，优先级和排序属于客户端。
- Rust 安全接口返回拥有所有权的快照；C ABI 返回独立不可变 handle，记录文本借用该 handle，
  到 free 前有效。读取不消费、不确认，后续 core 修改不改变已获取的快照。
  wire 解码经 `core/proto` 校验后复用同一快照读取语义；旧接口保留。
- 每 pane 最多 256 条；不为新 id 静默淘汰未确认终态。容量不足时拒绝新 id，
  已确认记录可淘汰，已有 id 可更新。OSC 保持 4096 字节整条上限；wire 每条 id/msg/app 的 UTF-8 字节和不超过 4096。
  非法 state/kind、控制字符、重复 id/seq、缺失/null/错类型、超量集合均拒绝。seq 耗尽后拒绝更新，不复用 ack 身份。

不需要重开 ADR-0003/0005/0006：新增能力与消息遵循既定的双外形、OSC 唯一来源和 CBOR 单点实现。
完整集合优先保证正确性；高频状态导致的重复编码/带宽仍受 256 条上限和出站队列限制。

## 已实现接口与读取语义

| 场景 | Rust 安全接口 | C ABI |
|---|---|---|
| 嵌入指定 pane | `Core::status_snapshot(pane_id)` | `pg_status_snapshot_new(core, pane_id)` |
| wire 消息读取 | `api::status_snapshot_of(&Message)` | `pg_msg_status_snapshot(msg)` |
| 集合与记录 | `StatusSnapshot { revision, records }`、`StatusSnapshotRecord` | `pg_status_snapshot_info`、`pg_status_snapshot_at` |
| 生命周期 | Rust 所有权 | `pg_status_snapshot_free` |

Rust 返回独立拥有的字符串；wire 接口另返回 pane id。C handle 可以跨线程只读，不能与 free 并发；
`PgStatusSnapshotInfo/Record.size` 由调用方初始化，小于当前结构体大小拒绝，大于时尾部清零。
读取出的 `PgStr` 借用 handle，复制文本须在 free 前完成。age_ms 固定在采样时，不随读取推进。
空集合是成功结果；非法 pane 返回 `NoSuchPane`，错消息类型、坏 wire 和越界索引返回 `InvalidArg`。
C 对应 NULL/false 并设置 `pg_last_error_code()`。`free(NULL)` 无副作用。

旧 `focused_status` / `pg_status_count/at` 仍枚举焦点 pane 的全部存储记录（包括 acked），
不带集合修订号；旧 C 文本借用 core，至下一次 core 修改前有效。新客户端应使用快照接口。
`ack_status` / `pg_status_ack` 成功意味着首次确认一个仍未确认的 done/error；重复、过期或 live ack 返回 false。

宿主通过 pane actor 串行采样与修改，广播模块一次编码新旧两种外形，由连接按能力选择。
ack、PTY 输出和直接子进程退出都调用同一广播入口。握手、attach、屏幕 since 续传和事件总线恢复
通过 pane 查询获取完整集合；status_all 观察者不加入尺寸投票。

## 客户端接入步骤

1. hello 请求 `status_snapshot`；需要侧栏观察全部 pane 时同时请求 `status_all`。
   仅在 hello_ack 确认能力后启用新路径，旧宿主未提供时继续读取旧 `status`。
2. `PG_MSG_STATUS_SNAPSHOT`（31）经 `pg_msg_status_snapshot` 得到 handle，读取 metadata 与全部记录并复制文本后 free。
   Rust 用 `status_snapshot_of`。不用在 Swift 实现 CBOR 或生命周期。
3. 新连接首次全量为该 pane 建立 revision 基准，随后只应用较新的 revision，原子替换集合。
   instance 变化清除旧基准，连接 generation 继续隔离旧回调；屏幕 seq 独立处理。
4. 从完整集合派生聚合与 UI 排序。记录不存在表示宿主已删除，不能据此持久化“本客户端已看过”。
   按已确认的可见性规则给具体记录发送 `ack_status {pane_id, seq}`；不得批量确认隐藏记录或 working/blocked。
5. sessions 删除 pane/session 时清理对应集合。实际自动确认仍需当前连接屏幕已呈现、窗口关注等条件，
   状态快照到达本身不构成查看证据。

## 本片验证（Linux，2026-11-04 至 2026-11-05）

验证对象为本 worktree 的 core、双外形读取和宿主协议，不包含 macOS/GTK UI 接入或跨平台端到端验收。

- 先验证安全接口和 C ABI 缺失的编译失败，以及真实宿主未协商新能力的断言失败，再实现对应切片。
- 默认 `cargo test -p pigeon-core --offline`：271 passed、0 failed、2 ignored；其后状态专题补充两例，
  `--test status_sync` 7/7 通过，库 130/130、宿主 bin 16/16、真实 PTY 状态 3/3 再验通过。
- `cargo test -p pigeon-core --no-default-features --offline`：185 passed、0 failed、1 ignored，包含上述 7 个状态专题测试。
  忽略项为既有 scroll 性能基准；默认 feature 另忽略仅无 internals 时适用的编译失败文档例。
- 默认/无默认 feature 的 `cargo clippy --all-targets -- -D warnings`、`cargo fmt --all --check`、
  `cargo build --lib --no-default-features`、`tools/checks/ffi-imports.sh` 通过。
- cbindgen 0.27 重新生成 C 头；独立再次生成 diff 为零，C11 编译语法检查通过。
- `cargo build -p pigeon-core --release --bin pigeon-server --offline`：宿主 1,838,624 字节，低于 8 MiB 上限。
- 覆盖：ack 立即揭露较低 seq 未读、多 blocked、OSC133 清 transient 保 done/error、clear 子树/空集合、
  进程退出、重连与 attach.since 全量、事件总线丢帧恢复、revision/record seq/屏幕 seq 分离、容量与非法 wire、
  旧能力兼容、未 attach 观察不投尺寸票、C handle 生命周期/size/索引/错误语义。

## 当前限制与父线程整合范围

- macOS/Linux 客户端尚未请求新能力；本片未修改 SidebarStore/AppDelegate。多记录显示、未读持久化和确认 UI
  仍需按以上步骤接入。共享 [优化计划](optimization-plan.md)、[当前缺口](plan.md) 和
  [持久化执行记录](status-persistence-progress.md) 由父线程合并更新。
- 这是宿主内存中的当前集合，不是历史事件日志或磁盘恢复。相同 id 的新报告替换旧报告；显式 clear 删除未读也生效。
  没有删除原因字段，不能从缺失推断本地用户查看；ack 属于宿主共享集合，多个客户端共同观察其结果。
- 完整快照有重复编码与传输成本；256 条上限约束记录集，出站仍受 64 MiB 断开阈值保护。
  容量满且没有已确认记录时忽略新 id；没有新增上游容量反馈或跨客户端持久化协调。
- 进程退出仅指宿主直接持有的 PTY child；不推测前台 agent 退出，不读取屏幕/标题推断状态。
- 本机未执行 macOS `swift build && swift test`、AppKit/SSH/真实 GUI 验收，Ubuntu 托管 CI 也未在本片运行。
  本片未更改 VT 语义，未重跑 VT 文本 golden；这些结果不替代父线程的平台验收。
