# ADR-0006: 协议是 u32 长度前缀 + CBOR，单点实现

- 状态：已采纳
- 日期：2026-10-27
- 依据：[protocol.md §1–2](../protocol.md)、[`core/src/proto/mod.rs`](../../core/src/proto/mod.rs)

## 决定

- **分帧**：`u32` 小端长度 + 恰好那么长的 payload，长度不含自身，单帧上限 8 MiB。
- **编码**：payload 是一个 CBOR map（RFC 8949），必带 `t: str` 作消息名。
  整数用最短编码，浮点只有 `ratio` 且用 float32，不发 null。
- **实现**：`core/src/proto/` 是唯一实现。客户端通过 `pg_msg_*` 编解码，Swift 侧零 CBOR。

## 约束

- 客户端不自己实现编解码；`ffi/msg.rs` 只许调 `api::msg`。
- 未知 key 忽略，未知 `t` 回 `unsupported`，不断开。
- `cell` 的 `attrs` 与颜色编码只加位、不改语义。
- 加字段要同时改 `proto/mod.rs`、`ffi/msg.rs`、`docs/protocol.md`，并有往返用例。

## 何时重开

需要零拷贝或流式大对象时。届时 bump `proto` 大版本。
