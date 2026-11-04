# Linux 客户端（M5 进行中）

GTK4 客户端，与 `clients/macos/` 对位。**Rust + `gtk4-rs`**，网格走 `GtkGLArea` + OpenGL ES 3 / 3.3 core。

**2026-11-04 更新**：首片可运行，但尚不具备 macOS 的完整日常操作与远端管理能力；当前缺口见下文。

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

快捷键（窗口级，先于 shell 生效）：`Ctrl+Shift+D` 左右分屏 · `Ctrl+Shift+E` 上下分屏 ·
`Ctrl+Shift+W` 关闭焦点 pane · `Ctrl+Shift+Z` 放大/还原 · `Ctrl+Shift+C` 复制焦点 pane 当前屏幕 · `Ctrl+Shift+V` 粘贴。

**已实现**：连接/自动拉起宿主、握手、每个 pane 一次 `attach`、一个 GLArea 画所有 pane（含分割线与 zoom）、
整屏网格渲染（背景/字形/下划线/删除线/反色/暗色/
光标三种形状）、R8 掩码图集 + RGBA 彩色图集（emoji）、CJK/全角/符号 fallback、制表符与方块元素
用矩形画（不接缝）、宽字符居中、xterm legacy 按键编码（含 DECCKM）、窗口缩放 → `resize_pane` → PTY 尺寸；协议 delta 更新（核心导出格宽，校验 seq/网格后原子应用，缺序重新 attach）。

**输入与剪贴板**：GtkIMMulticontext preedit/commit 与候选窗定位；异步剪贴板粘贴经宿主包装括号粘贴，切换 pane/失焦取消待完成粘贴。复制仅包含当前 pane 屏幕，保留附加码点和软换行，不含历史或选择。

**未做**：侧栏与会话选择、SSH 连接管理、自动断线恢复、拖动分隔线改比例、滚动回看、拖选/选择复制、
鼠标/焦点上报、Kitty keyboard protocol、渲染脏行增量、DMABUF、emoji ZWJ 序列整形。
GTK 客户端已有 Ubuntu CI 作业（构建/clippy/逻辑测试），尚无 GUI 发布产物。输入切片通过独立 Xvfb + fcitx5 拼音验收；Wayland、IBus、日文 IME、非美式键盘和混合 DPI 仍未验。见 [输入验证记录](../../docs/linux-input-progress.md)。
依赖 GTK4 开发库与 Rust ≥1.92（gtk4-rs 的最低工具链）；Ubuntu 可安装 `libgtk-4-dev pkg-config`。
详见 [docs/renderer.md](../../docs/renderer.md) §5.4。
