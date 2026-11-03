# ADR-0002: VT 引擎用现成实现，包在 `Term` trait 后

- 状态：已采纳
- 日期：2026-10-27
- 依据：[core.md §6](../core.md)、[`core/src/term/mod.rs`](../../core/src/term/mod.rs)

## 决定

依赖 `alacritty_terminal`（内部用 `vte` 解析字节），藏在 `Term` trait 后：
`feed` / `resize` / `snapshot` / `scroll`。`core/src/term/` 是实现 VT 语义的唯一地方。

## 约束

- 客户端不解析字节流，只搬字节与状态。
- 协议字段的语义来自 `Term` 接口，不来自引擎的具体类型。

## 已知限制

引擎不暴露 `scroll_region`，DECSTBM 滚动区恒等于整屏。
OSC 8 的 `hyperlink_id` 恒为 0，`cell[4]` 不上线。两条都记在 [plan.md §11](../plan.md)，不是 bug。

## 何时重开

引擎停止维护；或需要它不暴露的能力且绕不过去。换实现，不换接口。
