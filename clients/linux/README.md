# Linux 客户端（M5 进行中）

GTK4 客户端，与 `clients/macos/` 对位。**Rust + `gtk4-rs`**，网格走 `GtkGLArea` + OpenGL ES 3 / 3.3 core。

**2026-11-05 更新**：首片可运行，但尚不具备 macOS 的完整日常操作与远端管理能力；当前缺口见下文。

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
`Ctrl+Shift+W` 关闭焦点 pane · `Ctrl+Shift+Z` 放大/还原 · `Ctrl+Shift+C` 复制焦点 pane 当前可见屏幕（含回看屏幕） · `Ctrl+Shift+V` 粘贴。

回看快捷键：`Shift+PageUp/PageDown` 翻页（回看中裸 `PageUp/PageDown` 也翻页）；`Shift+Home` 到最旧历史、
`Shift+End` 或 “Return to live” 按钮回实时屏。裸 PageUp/PageDown 在实时屏上仍发给程序；Ctrl/Alt 修饰不触发回看。
滚轮每档三行，触控板按行高累积；操作焦点 pane。备用屏不回看，PageUp/PageDown 发给程序，
滚轮仅在 `?1007` 开启且没有鼠标上报模式时发送方向键。鼠标协议上报仍未实现。
输入（含 IME preedit 开始）、有效粘贴或 resize/zoom 改变网格尺寸时回实时屏；切 pane 保留各自回看位置。

**已实现**：连接/自动拉起宿主、握手、每个 pane 一次 `attach`、一个 GLArea 画所有 pane（含分割线与 zoom）、
整屏网格渲染（背景/字形/下划线/删除线/反色/暗色/
光标三种形状）、R8 掩码图集 + RGBA 彩色图集（emoji）、CJK/全角/符号 fallback、制表符与方块元素
用矩形画（不接缝）、宽字符居中、xterm legacy 按键编码（含 DECCKM）、窗口缩放 → `resize_pane` → PTY 尺寸；协议 delta 更新（核心导出格宽，校验 seq/网格后原子应用，缺序重新 attach）。

**输入与剪贴板**：GtkIMMulticontext preedit/commit 与候选窗定位；异步剪贴板粘贴经宿主包装括号粘贴，切换 pane/失焦取消待完成粘贴。复制仅包含当前 pane 屏幕，保留附加码点和软换行，不含可见屏幕之外的历史或选择。

**未做**：侧栏与会话选择、SSH 连接管理、自动断线恢复、拖动分隔线改比例、拖选/选择复制、
鼠标/焦点上报、Kitty keyboard protocol、渲染脏行增量、DMABUF、emoji ZWJ 序列整形。
GTK 客户端已有 Ubuntu CI 作业（构建/clippy/逻辑测试），尚无 GUI 发布产物。输入切片通过独立 Xvfb + fcitx5 拼音验收；Wayland、IBus、日文 IME、非美式键盘和混合 DPI 仍未验。见 [输入验证记录](../../docs/linux-input-progress.md)。

**回看**：握手协商 `scrollback`，通过宿主 `scroll` 获取一屏（不拉整段历史），显示回看位置与每 pane 位置条。
实时 delta/快照不覆盖回看；快速滚动采用最后答复，回到底部后恢复 delta 序号校验。
独立 Xvfb + 真实宿主通过 10,000 行回看、延迟答复、输入/IME/粘贴、resize、备用屏与分屏验收；
真实触控板与 Wayland 未验；每 15 秒 ping 保持长时间阅读连接，自动断线恢复仍未实现。
见 [回看验证记录](../../docs/linux-scroll-progress.md)，本切片不代表 S5 完成。

依赖 GTK ≥4.8 开发库与 Rust ≥1.92（gtk4-rs 的最低工具链）；Ubuntu 可安装 `libgtk-4-dev pkg-config`。
详见 [docs/renderer.md](../../docs/renderer.md) §5.4。
