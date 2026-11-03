# ADR-0004: `pigeon-server` 持有 PTY，会话活过窗口

- 状态：已采纳
- 日期：2026-10-27
- 依据：[plan.md §3.3](../plan.md)、[`core/src/bin/pigeon-server/`](../../core/src/bin/pigeon-server)

## 决定

`pigeon-server` 是用户看不见的后台进程，持有每个 pane 的 PTY 与屏幕状态，
在 `/tmp/pigeon-<uid>.sock`（0700）上听客户端。客户端只观察，不持有 PTY。

连不上 socket 时，客户端跑 `pigeon-server --detach`：在新的进程组里拉起宿主，等 socket 能连再返回。
宿主不是窗口的子进程。不写 LaunchAgent、不注册登录项。

`pigeon-server` 打包在 `Pigeon.app/Contents/Helpers/` 里，本机与远端跑同一个二进制。

## 约束

- 客户端不许碰 PTY；`pigeon-server` 里不许出现 UI 概念。
- 嵌入模式与宿主模式跑同一份内核，只有一份快照格式、一套 OSC 7501 规则。
- 结束全部会话走 `shutdown` 消息，不靠杀进程。
- 宿主结束时会话全部消失，不写盘。

## 何时重开

不重开。
