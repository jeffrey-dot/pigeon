# ADR-0003: 一份语义、两种外形：`pigeon_core::api` + C ABI

- 状态：已采纳
- 日期：2026-10-27
- 依据：[core.md §4.1](../core.md)、[`core/src/api/`](../../core/src/api)、[`core/src/ffi/`](../../core/src/ffi)

## 决定

- `core/src/api/`：安全的 Rust API，分组与 C ABI 一一对应（pane / 快照 / 状态 / 布局 / 消息），
  所有权表达生命周期，错误是 `Result<T, PgError>`。
- `core/src/ffi/`：`api` 的薄包装，只做指针/长度转换、错误码、`catch_unwind`，不实现语义。
- `term` / `layout` / `status` / `session` / `proto` 只在 `internals` feature 下是 `pub`；
  客户端以 `default-features = false` 依赖时它们是私有模块。

## 约束

- `ffi` 只许调 `api`，不直接碰内部模块。
- 两种外形同步改：给 C ABI 加函数就同时加 `api` 的对应项，反之亦然。
- `core/tests/abi.rs` 测 C 外形，`core/tests/api.rs` 测 Rust 外形。

## 何时重开

出现第三种外形（WASM、C++ 包装）时**扩展**，不合并成一种。
