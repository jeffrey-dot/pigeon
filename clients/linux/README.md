# Linux 客户端（M5 进行中）

GTK4 客户端，与 `clients/macos/` 对位。**Rust + `gtk4-rs`**，网格走 `GtkGLArea` + OpenGL ES 3 / 3.3 core。

**2026-11-05 更新**：已接本地多会话、GTK 原生侧栏和逐条状态确认；仍不具备 macOS 的完整日常操作与远端管理能力；当前缺口见下文。

只依赖 `pigeon_core::api`（`default-features = false`）；不 import `PigeonApp`，不进 `core/`。
渲染形状见 [docs/renderer.md](../../docs/renderer.md)，选型理由见
[docs/adr/0011](../../docs/adr/0011-linux-rendering.md)，交互规格见 [docs/ui.md](../../docs/ui.md)，
协议见 [docs/protocol.md](../../docs/protocol.md)。

```sh
cargo build -p pigeon-linux                  # 从仓库根（或 cd clients/linux && cargo build）
cargo run   -p pigeon-linux                  # 自动拉起本机 pigeon-server，列出会话并附加选中会话
PIGEON_FRAME_STATS=1 cargo run -p pigeon-linux   # 打印 GL 版本与每秒帧统计
PIGEON_FONT="Fira Code" cargo run -p pigeon-linux  # 换字体（默认按固定名单挑）
cargo test -p pigeon-linux                   # 纯逻辑单测（会话路由 / 状态集合 / 按键 / 回看 / 渲染）
```

环境变量：`PIGEON_SERVER` 指定 `pigeon-server` 路径；`PIGEON_FONT` 指定字体族；
`PIGEON_FRAME_STATS=1` 打开帧统计；`PIGEON_FONT_DEBUG=1` 打印选中的字体。

会话操作：顶部 `+`（**New Session**）在选中会话目录新建，悬停会话行右侧 `×`（**Close Session…**）确认后关闭该会话的全部 panes；
点击侧栏会话恢复终端焦点。`Ctrl+Shift+T` 新建，`Ctrl+Shift+[` / `]` 循环切换，
`Ctrl+Shift+B` 隐藏/显示侧栏，`Ctrl+Shift+J` 循环选择 needs-you 会话并打开记录查看器。
首次空宿主自动建会话；关闭最后一个会话后保留空窗口，可用 New Session 再建。关闭窗口不关闭宿主/PTY。

快捷键（终端焦点下，先于 shell 生效）：`Ctrl+Shift+D` 左右分屏 · `Ctrl+Shift+E` 上下分屏 ·
`Ctrl+Shift+W` 关闭焦点 pane · `Ctrl+Shift+Z` 放大/还原 · `Ctrl+Shift+C` 复制焦点 pane 当前可见屏幕（含回看屏幕） · `Ctrl+Shift+V` 粘贴。

回看快捷键：`Shift+PageUp/PageDown` 翻页（回看中裸 `PageUp/PageDown` 也翻页）；`Shift+Home` 到最旧历史、
`Shift+End` 或 “Return to live” 按钮回实时屏。裸 PageUp/PageDown 在实时屏上仍发给程序；Ctrl/Alt 修饰不触发回看。
滚轮每档三行，触控板按行高累积；操作焦点 pane。备用屏不回看，PageUp/PageDown 发给程序，
滚轮仅在 `?1007` 开启且没有鼠标上报模式时发送方向键。鼠标协议按核心模式上报，回看期间保持本地操作；见 [现代输入](../../docs/linux-modern-input.md)。
输入（含 IME preedit 开始）、有效粘贴或 resize/zoom 改变网格尺寸时回实时屏；切 pane 保留各自回看位置；**切会话明确请求回实时屏**，同时取消 preedit 和待完成粘贴。

**已实现**：连接/自动拉起宿主、握手、仅为选中会话 `attach`（切回时重新建立布局订阅）、一个 GLArea 画所有 pane（含分割线与 zoom）、
整屏网格渲染（背景/字形/下划线/删除线/反色/暗色/
光标三种形状）、R8 掩码图集 + RGBA 彩色图集（emoji）、CJK/全角/符号 fallback、制表符与方块元素
用矩形画（不接缝）、宽字符居中、xterm legacy/Kitty 按键编码（含 DECCKM/DECKPAM）、窗口缩放 → `resize_pane` → PTY 尺寸；协议 delta 更新（核心导出格宽，校验 seq/网格后原子应用，缺序重新 attach）。

**输入与剪贴板**：GtkIMMulticontext preedit/commit 与候选窗定位；异步剪贴板粘贴经宿主包装括号粘贴，切换 pane/失焦取消待完成粘贴。复制仅包含当前 pane 屏幕，保留附加码点和软换行，不含可见屏幕之外的历史或选择。

**本地侧栏与状态**：按本地 git 根/cwd 分组、列出所有会话、紧急度排序、未读记录数、needs-you 会话计数及尺寸来源。
无 OSC 7501 报告时不给状态或虚构活动时间。hello 请求 `status_all` + `status_snapshot`，仅按协商路径读取；
后台状态观察不 attach 未访问 pane，不为它们投尺寸票。已访问会话受现协议无 detach 的限制仍留有屏幕订阅/尺寸票，
客户端只 resize 当前画面。完整集合按本连接、宿主 instance 和 pane 的 revision 原子替换。

**Status Records…** 可逐条查看 pane、id、state、app、kind、完整纯文本 msg；选择记录会聚焦来源 pane并解除隐藏它的 zoom，
不会自动退出回看。仅 done/error 可点 **Mark This Record as Read**；确认要求当前连接屏幕已绘制、目标 pane 可见且实时、窗口前台。
后台/隐藏/回看不确认，working/blocked/idle 不 ack。切会话、断线、记录被替换/clear 后旧确认目标失效。
旧宿主仅显示实际发来的单条报告，保留已收到的旧终态；同 id 新报告撤销旧身份，已确认记录可回收；不保证发现旧宿主未发送的记录，长期未确认集合的资源预算待补。
无跨客户端重启 seen 持久化、无 SSH、无自动断线恢复。详见 [侧栏验证记录](../../docs/linux-sidebar-progress.md)。

**未做**：SSH 连接管理、自动断线恢复、拖动分隔线改比例、拖选/选择复制、
渲染脏行增量、DMABUF、emoji ZWJ 序列整形。
GTK 客户端已有 Ubuntu CI 作业（构建/clippy/逻辑测试），尚无 GUI 发布产物。输入切片通过独立 Xvfb + fcitx5 拼音验收；Wayland、IBus、日文 IME、非美式键盘和混合 DPI 仍未验。见 [输入验证记录](../../docs/linux-input-progress.md)。

**回看**：握手协商 `scrollback`，通过宿主 `scroll` 获取一屏（不拉整段历史），显示回看位置与每 pane 位置条。
实时 delta/快照不覆盖回看；快速滚动采用最后答复，回到底部后恢复 delta 序号校验。
独立 Xvfb + 真实宿主通过 10,000 行回看、延迟答复、输入/IME/粘贴、resize、备用屏与分屏验收；
真实触控板与 Wayland 未验；每 15 秒 ping 保持长时间阅读连接，自动断线恢复仍未实现。
见 [回看验证记录](../../docs/linux-scroll-progress.md)，本切片不代表 S5 完成。

依赖 GTK ≥4.8 开发库与 Rust ≥1.92（gtk4-rs 的最低工具链）；Ubuntu 可安装 `libgtk-4-dev pkg-config`。
详见 [docs/renderer.md](../../docs/renderer.md) §5.4。

**主界面**：Harp 风格单行侧栏、状态图标、白色/近黑终端卡片与会话顶部栏；记录详情通过顶部小图标按需打开。`PIGEON_APPEARANCE=light|dark` 固定外观，否则跟随 GTK 深色偏好。默认优先 DejaVu Sans Mono，`PIGEON_FONT` 可覆盖。复跑截图与平台限制见 [Linux Harp 主界面](../../docs/linux-harp.md)。

**现代输入与链接**：Kitty 五个 flag、事件配对、应用小键盘、鼠标/焦点报告已接；OSC 8 纯文本 hover 与完整同格 Ctrl+左键打开 http/https，拖动/滚动/失焦/画面变化取消。规范、回归和硬件限制见 [现代输入与链接](../../docs/linux-modern-input.md)。
