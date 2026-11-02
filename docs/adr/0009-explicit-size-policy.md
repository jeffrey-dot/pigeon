# ADR-0009: 多客户端尺寸策略显式且可见

- 状态：已采纳
- 日期：2026-10-28
- 依据：[protocol.md §6](../protocol.md)、[`core/src/session/size_policy.rs`](../../core/src/session/size_policy.rs)

## 决定

策略是 **pane 级**的，三种：`latest`（默认，最近一次发 `focus`/`keys`/`mouse` 的客户端说了算）、
`smallest`（取所有已 attach 客户端的最小值）、`locked`（只有首次设策略的客户端能改）。

由第一个 `attach` 决定，后续 `attach` 只确认；改策略要重新 `attach`。
每次尺寸变化广播 `size_state{pane_id, cols, rows, source, clients}`。

## 约束

- 客户端必须把 `source` 显示在侧栏（`AGENTS.md` 不变量 6）。
- 尺寸变化必须广播 `size_state`，不许只改屏幕。
- 只看状态的连接（`status_all`）不参与尺寸投票。

## 何时重开

做到每客户端独立尺寸时，新增 `per_client` 策略，不改已有三种的语义。
