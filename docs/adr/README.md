# 架构决策记录（ADR）

> 记**扭转成本高**的决定：选了哪个依赖、语言、协议格式、线程模型，以及划在哪的硬边界。
> 代码说做了什么，`docs/` 说怎么用，这里说为什么定了这一种。

## 怎么写

- 一个文件一个决定，`NNNN-短语.md`（四位数字，小写，连字符）。
- 状态只有四种：`已采纳` / `已被 ADR-NNNN 取代` / `已废弃` / `提议中`。
- 已采纳的 ADR 不再改内容。要改判写新的一条，旧条标「已被取代」并互相链接。
- 中文。标识符与协议字段保持英文原样。

## 模板

```markdown
# ADR-NNNN: <一句话决定>

- 状态：已采纳
- 日期：YYYY-MM-DD
- 依据：<落地时改的文件或文档>

## 决定

选了什么，边界划在哪。

## 约束

因此不许做什么。每条一句，可被评审引用。

## 何时重开

可观测的触发条件；没有就写「不重开」。
```

## 索引

| ADR | 决定 | 日期 |
|---|---|---|
| [0001](0001-rust-kernel-c-abi.md) | 内核用 Rust，C ABI 是唯一的跨语言缝 | 2026-10-27 |
| [0002](0002-vt-engine-behind-term-trait.md) | VT 引擎用现成实现，包在 `Term` trait 后 | 2026-10-27 |
| [0003](0003-one-semantics-two-shapes.md) | 一份语义、两种外形：`pigeon_core::api` + C ABI | 2026-10-27 |
| [0004](0004-host-owns-pty.md) | `pigeon-server` 持有 PTY，会话活过窗口 | 2026-10-27 |
| [0005](0005-status-only-from-osc-7501.md) | 状态只来自 OSC 7501 | 2026-10-27 |
| [0006](0006-length-prefixed-cbor-protocol.md) | 协议是 u32 长度前缀 + CBOR，单点实现 | 2026-10-27 |
| [0007](0007-platform-native-clients.md) | 客户端按平台原生，macOS AppKit + Metal 首发 | 2026-10-27 |
| [0008](0008-remote-over-system-ssh.md) | 远端经系统 ssh，推送静态 server（a/b/c 三档） | 2026-10-27 |
| [0009](0009-explicit-size-policy.md) | 多客户端尺寸策略显式且可见 | 2026-10-28 |
| [0010](0010-gpl-3-0-or-later.md) | 许可证 GPL-3.0-or-later | 2026-10-27 |

「还没做的事」在 [plan.md §11](../plan.md) 的已知缺口里，不在这。
