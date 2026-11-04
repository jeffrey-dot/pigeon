# Linux 日常输入切片

2026-11-04，基于 `491aeb681e4298f7bcd119e02971e07a76d9850d`。
对应 [优化计划](optimization-plan.md) S5 与首批 Linux 输入任务。
本记录覆盖本切片实现和验证；共享 [当前缺口](plan.md#11-当前能力与已知缺口)、
[Linux README](../clients/linux/README.md) 与优化执行记录由父线程在合并切片时统一同步。

## 用户行为

- `GtkIMMulticontext` 接收按下和释放事件；preedit 在 GTK 纯文本浮层显示，包含插入点，commit 才发送 UTF-8 `Message::Keys`。
  窗口快捷键先处理；IME 消费或同步提交过的事件不再走 legacy 编码，普通键、Ctrl/Alt 与方向键保持原编码。
- 候选窗位置使用焦点 pane 的宿主光标、渲染格子度量、pane 原点和设备缩放换算为 GTK 逻辑像素。
  快照、delta、resize、zoom 和点击切换 pane 均更新位置。失焦、切换 pane、pane 退出或控件销毁清除 preedit。
- `Ctrl+Shift+V` 异步读取系统剪贴板文本，发 `Message::Paste`；不在客户端添加括号标记、转换换行或直接操作 PTY。
  宿主依照当前 `?2004` 模式包装并过滤正文中的起止标记。空文本不发送；无文本或读取错误响铃，错误写入日志。
  异步读取期间若焦点/目标变化则丢弃结果，避免把文本发送到错误 pane。
- `Ctrl+Shift+C` **复制焦点 pane 当前可见屏幕**，不含历史、其它 pane，也不是选择复制。
  使用核心 `Cell::text()` 保留附加码点、跳过宽字符续格；按核心 `ATTR_WRAP` 连接软换行，保留硬换行/空行，去掉硬行尾空格。
  GTK tooltip 明示范围。GDK 发布本地文本 provider，由 GTK 异步完成跨应用传输；没有主线程阻塞剪贴板 IO。
- 原 `Ctrl+Shift+D/E/W/Z` 分屏/关闭/zoom 快捷键支持 GTK 的大小写 keyval，额外 Alt/Super/Meta 不误触发。
  布局关闭的 pane 从客户端缓存移除；新焦点 pane 快照未到时不把输入回退发送到旧 pane。

## 实现范围

改动仅在 Linux 输入/UI 和客户端测试：`input_context.rs` 管 GTK 输入与剪贴板生命周期，
`input.rs` 保持纯输入映射，`view/input.rs` 负责 workspace 输入目标和窗口动作；
原位置映射移到 `placement.rs`，所有新增/修改 Rust 文件均低于 400 行。
`host.rs` 使用无界通道的 `try_send` 入队，socket 编码/写入仍在原工作线程。
未改 `core/`、`render/`、协议或生成头文件；未新增依赖。

## 验证

环境：Arch Linux，Rust 1.99.1，GTK 4.24.1，gtk4-rs 0.11.5，fcitx5 5.1.23（拼音），
Xvfb X11，Mesa 26.2.6 的 OpenGL ES 3.2 软件渲染，debug 构建，默认字体，初始窗口 640×408，缩放 1。

- 先运行行为测试：21 passed / 6 failed，确认快捷键大小写、IME 去重、空 commit、粘贴消息类型、复制文本边界的预期失败。
  实现后 `cargo test -p pigeon-linux`：**28 passed**（含既有渲染纯逻辑测试）。
- `cargo fmt -p pigeon-linux --check`、`cargo clippy -p pigeon-linux --all-targets -- -D warnings`、
  `cargo build -p pigeon-linux`：通过。另构建 `pigeon-server` 用于实测。
- [隔离桌面测试](../clients/linux/tests/input_smoke.py)：**通过，退出码 0**。它自行创建 Xvfb、D-Bus、HOME、XDG 目录、
  fcitx5 配置和宿主 socket；宿主的测试程序只记录 stdin 字节，用户 shell rc、已有终端、剪贴板和输入法实例均不参与。
  覆盖普通键/Ctrl/Alt/方向键精确字节、拼音 preedit 零字节与“你好”单次提交、括号粘贴和正文标记过滤、
  普通粘贴/CRLF/空剪贴板、组合重音/CJK 复制、分屏/zoom/关闭/焦点恢复、切换 pane 取消 preedit 与待完成粘贴、
  窗口失焦取消 preedit；无 GTK critical 或 Rust panic。
- 人工检查同一隔离测试生成的截图：单 pane 与分屏后移动至第 5 行第 10 列，preedit 与候选窗均跟随光标。
  截图检查发现并修正了 GTK margin 被重复计入 preedit 测量导致的偏移。
- 文档本地链接检查与 `git diff --check`：通过。

复跑命令（桌面脚本另需 `python-gobject`、`Xvfb`、`xdotool`、`dbus-run-session`、fcitx5 拼音与 GTK4 IM module）：

```sh
cargo build --bin pigeon-server
cargo build -p pigeon-linux
python3 clients/linux/tests/input_smoke.py
# 可选保存截图，需 ImageMagick import；此脚本只截图独立 Xvfb。
PIGEON_INPUT_SCREENSHOT=/tmp/pigeon-input-preedit.png python3 clients/linux/tests/input_smoke.py
```

## 未验与限制

- 本片没有回看、拖选/选择模型、鼠标/焦点协议上报、Kitty keyboard protocol、OSC 52，也不宣称这些能力完成。
- 未完成真实 Wayland/硬件桌面、IBus、日文 IME、死键/非美式键盘、混合 DPI/分数缩放或跨屏候选窗验收。
  2× 坐标换算有纯逻辑测试，不能替代桌面实测。
- preedit 使用统一下划线和插入点，尚不还原 IME 返回的分段颜色/样式；长 preedit 在窗口边缘截断/省略。
  没有 surrounding-text 编辑协议；终端程序拥有真实编辑缓冲，客户端不伪造该缓冲。
- GDK 异步读取没有本片新增的大小上限或超时；超大粘贴仍受既有协议帧与宿主输入预算限制。
  未做大剪贴板压力、慢 provider 超时、多次粘贴顺序、断线或长期 soak 验收。
- 隔离桌面会打印 Xvfb DRI3/桌面 portal 不可用的环境警告；不影响上述软件渲染和输入断言。
  未运行 macOS 构建/测试或托管 Ubuntu CI；本片无核心语义修改，未复跑全核心/VT 对账。
