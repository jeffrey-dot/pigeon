# Linux 客户端（M5 进行中）

GTK4 客户端，与 `clients/macos/` 对位。**Rust + `gtk4-rs`**，网格走 `GtkGLArea` + OpenGL 3.3。

只依赖 `pigeon_core::api`（`default-features = false`）；不 import `PigeonApp`，不进 `core/`。
渲染形状见 [docs/renderer.md](../../docs/renderer.md)，选型理由见
[docs/adr/0011](../../docs/adr/0011-linux-rendering.md)，交互规格见 [docs/ui.md](../../docs/ui.md)，
协议见 [docs/protocol.md](../../docs/protocol.md)。

```sh
cargo build -p pigeon-linux                  # 从仓库根（或 cd clients/linux && cargo build）
cargo run   -p pigeon-linux                  # 自动拉起本机 pigeon-server 并接上第一个 pane
PIGEON_FRAME_STATS=1 cargo run -p pigeon-linux   # 打印 GL 版本与每秒帧统计
PIGEON_FONT="Fira Code" cargo run -p pigeon-linux  # 换字体（默认按固定名单挑）
cargo test -p pigeon-linux                   # 纯逻辑单测（按键编码 / 调色板 / 实例布局）
```

环境变量：`PIGEON_SERVER` 指定 `pigeon-server` 路径；`PIGEON_FONT` 指定字体族；
`PIGEON_FRAME_STATS=1` 打开帧统计；`PIGEON_FONT_DEBUG=1` 打印选中的字体。

**已实现**：连接/自动拉起宿主、握手、`attach`、整屏网格渲染（背景/字形/下划线/删除线/反色/光标三种形状）、
字形图集、CJK 与全角 fallback、xterm legacy 按键编码（含 DECCKM）、窗口缩放 → `resize_pane` → PTY 尺寸。

**未做**：侧栏、分屏、滚动回看、选择/复制、IME、鼠标上报、emoji 彩色图集、DAMAGE 增量、DMABUF。
详见 [docs/renderer.md](../../docs/renderer.md) §5.4。
