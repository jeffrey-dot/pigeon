# Linux 客户端（M5，未开工）

GTK4 客户端，与 `clients/macos/` 对位。今天这里没有代码。

只依赖 `pigeon_core::api`（Rust）或 `include/pigeon_core.h`（C ABI）；不 import `PigeonApp`，
不进 `core/`。交互规格见 [docs/ui.md](../../docs/ui.md)，协议见 [docs/protocol.md](../../docs/protocol.md)。
