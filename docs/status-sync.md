# 同 pane 多记录状态同步

设计日期：2026-11-04；基线 `cc51ce22aec38519ef038b6ac7f2d8791c36ccc2`。
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
  已确认记录可淘汰，已有 id 可更新。输入和 wire 记录限制及失败方式在实现后补齐。

不需要重开 ADR-0003/0005/0006：新增能力与消息遵循既定的双外形、OSC 唯一来源和 CBOR 单点实现。
完整集合优先保证正确性；高频状态导致的重复编码/带宽仍受 256 条上限和出站队列限制。

## 实施与验证

当前为待实施设计。交付时补实际接口、测试结果和接入步骤；共享 plan/progress 由父线程整合。
