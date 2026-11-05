# Linux Harp 主界面

2026-11-05 按用户再次提供的 [Harp 参考图](images/reference-harp-layout.png) 核对。Linux 主界面采用与 macOS 相同的信息层级与色值，原生窗口控制仍遵循 Linux/GTK 习惯。

- 272 逻辑像素浅灰侧栏，顶部仅有记录、折叠和新建图标；无常驻的大操作按钮或记录正文。
- 项目组头显示名称、小号数量和橙色 needs-you；按项目首次会话顺序排列，同组状态排序规则保留。
- 单行会话为名称、时间、16px 状态槽位；未读名称半粗，选中白色圆角薄边，悬停关闭按钮替换状态槽位。
  名称优先使用宿主 active_title，全部纯文本；pane 编号、任务细节、尺寸来源留在提示中。
- 右侧为圆角终端卡片；顶部显示会话名、目录和当前状态，终端与卡片背景一致，保留 28px 左内边距。
- Status Records 按需在终端卡片上浮出，不挤占侧栏和网格尺寸。显式确认仍要求具体记录、当前已绘制实时屏、前台窗口及可见 pane。
- 浅色终端白底、深色近黑；ANSI 色表与 macOS 相同。`PIGEON_APPEARANCE=light|dark` 可固定外观，否则读取 GTK 的深色偏好并监听变化。
  默认优先 DejaVu Sans Mono，`PIGEON_FONT` 仍可覆盖；尚无完整字体配置/热重载 UI。
- 颜色通过既有 Appearance 消息告诉宿主，使程序颜色查询得到相同背景；不重排终端转录，不伪造程序内容。

渲染修复包括保持不透明 framebuffer 的 alpha，避免浅色文字被二次合成冲淡；普通单格字形按基线左边距绘制，不按各字形位图宽度额外居中。宽字符继续按格宽处理。标题栏带来的 GDK surface 偏移参与物理鼠标坐标转换，使移动与按下/释放一致。

## 验证

```sh
cargo build -p pigeon-linux --locked
cargo build --example keys --bin pigeon-server --locked
python3 -B tools/checks/linux-harp-check.py
python3 -B tools/checks/linux-input-check.py
python3 -B tools/checks/linux-scroll-check.py
python3 -B tools/checks/linux-modern-input-check.py
python3 -B tools/checks/linux-sidebar-check.py
tools/checks/linux-gl-check.sh
```

截图脚本在独立 Xvfb、HOME、配置、D-Bus、宿主与真实 PTY 中播入明确标为 fixture 的输出，生成 light/dark.png。
默认产物目录 `/tmp/pigeon-linux-harp`，可用 `PIGEON_HARP_ARTIFACTS` 指定。不是 HTML mockup，也不会操作用户桌面或改 shell 配置。

Arch Linux/GTK 4.24.1/Mesa 软件 GL 下已检查浅色/深色截图；输入、回看、现代键鼠与链接、侧栏状态/确认脚本通过。
GL 混合状态回归先失败后修复。GUI 脚本坐标更新到新卡片原点，仍验证实际协议字节、复制文本与正确 pane。
尚未验真实 Wayland/混合 DPI、硬件 GPU、Orca 完整可访问性；侧栏拖动宽度、重命名和精确 macOS 材质不是本片已实现能力。
