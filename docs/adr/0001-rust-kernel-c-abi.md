# ADR-0001: 内核用 Rust，C ABI 是唯一的跨语言缝

- 状态：已采纳
- 日期：2026-10-27
- 依据：[core.md §6](../core.md)、[`core/src/ffi/mod.rs`](../../core/src/ffi/mod.rs)

## 决定

内核用 Rust。`cbindgen` 从 `core/src/ffi/mod.rs` 生成 `include/pigeon_core.h`，
这是对非 Rust 客户端的唯一接口；Rust 客户端走 `pigeon_core::api`（[ADR-0003](0003-one-semantics-two-shapes.md)）。

## 约束

- `core/` 只用 std + POSIX 绑定：不引平台专属 crate，不出 `cfg(target_os = …)` 分支
  （PTY 与信号除外，且两边都要有实现）。
- `core/src/` 不出 `unsafe`，唯一例外是 `core/src/ffi/`。
- `core/` 不碰窗口、字体、图形 API。
- `include/pigeon_core.h` 是生成物：改 ABI 就改 `core/src/ffi/`。

## 何时重开

`musl` 静态链接撑不住 `pigeon-server` 的 8 MB 上限；或 cbindgen 覆盖不了某个必要能力。
