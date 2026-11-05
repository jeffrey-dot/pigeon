# Linux 现代输入与 OSC 8 链接

核对日期：2026-11-05；基线 `03b1a41e4856d5ec73245b6f34f97445fade2557`。
本片只改 Linux 客户端、独立验收脚本和本文，对应 [优化计划](optimization-plan.md) S3/S5 的输入部分。
不代表 Linux 功能对齐完成；集中能力与后续缺口由整合任务维护在 [plan.md](plan.md#11-当前能力与已知缺口)。
此前切片证据见 [输入](linux-input-progress.md)、[回看](linux-scroll-progress.md)、[侧栏](linux-sidebar-progress.md)。

## 当前行为

### 键盘与输入法

- 客户端读取 snapshot/delta 的 `modes`，按 [协议 §3.3](protocol.md#33-屏幕) 位 10–14 映射 Kitty 五个 flag：
  消歧、事件类型、备用键、全部按键转义、附带文本。协商栈、查询应答和 VT 解析仍由核心完成。
- `keyboard.rs` 和 `keyboard/table.rs` 只编码客户端输入：Enter/Tab/Backspace/Escape、方向/导航、F1–F35、
  小键盘及左右 Shift/Ctrl/Alt/Super/Hyper/Meta。按实际 flag 生成 press/repeat/release；未启用事件类型时重复等同按下。
  未要求全部按键转义时保留普通文本和未修饰 Enter/Tab/Backspace，不发送其 release；Ctrl+I 可与 Tab 区分。
- 按硬件 keycode 记录实际发出的按下、协议状态和键身份。IME 消费、窗口快捷键和回看快捷键不产生孤立 release。
  先松 Ctrl 再松字符仍发送对应字符 release；修饰键字段使用释放时状态。失焦、切 pane/会话、进入回看时结束仍有效的已发按键。
  协议 flag 已改变时丢弃旧 release；释放本身不触发回实时屏。按下已排在回实时屏请求之后时，release 仍按序入队。
- GDK 当前 layout 的无修饰映射提供 unshifted code，Shift 后实际 keyval 提供 shifted code；仅当 GDK keymap
  提供该硬件键的 ASCII layout 时填写 base code，不假设 US 标点或凭硬件位置猜字母。缺失时省略备用字段。
  Caps Lock 与左右修饰键有映射；GDK4 公共 ModifierType 不提供 Num Lock 状态，因此不报告 Num Lock modifier bit。
- `GtkIMMulticontext` 仍先处理事件。同步普通单字符 commit 可附到该键；preedit/合成文本只提交一次，
  Kitty 全键+文本模式使用 `CSI 0 ;; codepoints u` 表示无物理键身份的文本事件，不再发送确认候选键的 release。
  仅全键模式、未请求附带文本时不发送无法关联物理键的 IME 文本；普通模式保持 UTF-8 中文单次 commit。
- legacy 继续遵循 DECCKM，新增 DECKPAM 小键盘 `SS3` 编码，保留 Shift+Tab。异步剪贴板仍发 `Message::Paste`，
  由宿主包装括号粘贴；切 pane/会话/失焦取消待完成粘贴。复制仍是焦点 pane 当前可见屏幕，不是拖选。

### 鼠标、焦点与回看

- xterm `?1000` 点击、`?1002` 拖动、`?1003` 移动，配合 `?1006` SGR；未启用 SGR 时使用传统字节格式，
  超过 223 的坐标拒绝编码。坐标按实际 pane 原点、格子度量和缩放映射，wire 为一基坐标。
  Shift/Alt/Ctrl 修饰位、左/中/右键按下/释放与垂直滚轮通过 `Message::Keys` 发送。GTK 物理 release 不受点击手势拖动阈值影响。
- 按下已发送才有释放，链接手势消费整次鼠标点击。滚轮每档三行/三次报告，surface 位移按逻辑行高累积。
  `?1004` 只向当前有效输入 pane 报告焦点变化；窗口失焦、控件焦点转到侧栏和 pane 切换均跟踪。
- 回看或回看答复未完成时不发送鼠标/焦点报告，不把历史点击/滚轮当作程序操作；每 pane 回看仍独立。
  滚轮无鼠标模式时继续本地回看；备用屏仅在 `?1007` 开启时转方向键，遵循 DECCKM。
  键入、有效 IME 和粘贴仍回实时屏，侧栏选择尚未建立布局/新屏幕时不接受输入。

### 链接

- 直接使用核心 `Cell.hyperlink`，不扫描文字或复制 OSC 8 解析。有效目标 hover 使用 GTK **纯文本** tooltip
  `Ctrl-click to open: …`；Ctrl 改变时同步鼠标手形。
- 仅明确 Ctrl+左键按下并在同 pane、同格、同 URI、同画面 revision 抬起才调用 GIO 系统 URI opener。
  移动（包括同格内移动）、离开、失焦、滚动、切 pane/会话、画面更新、目标消失均取消；取消后即便恢复相同 URI 也不重新激活。
  链接必须属于已绘制的当前画面，刚收到但尚未显示的链接不能打开。系统调用不经 shell，不拼接命令。
- 与 [OSC 8 打开策略](osc8.md#macos-交互与打开策略) 一致，只允许有 host 的 http/https，最长 2048 字节；
  拒绝 credentials、空/非法/零/超界端口、反斜杠、控制/空白/bidi 字符、无效百分号或 UTF-8、编码后控制字符。
  路径 `%20` 允许；authority 中的百分号编码保守拒绝。其它 scheme 只保留核心元数据，不调用 opener。

## 快捷键

| 操作 | 快捷键 |
|---|---|
| 复制当前可见屏幕 / 异步粘贴 | `Ctrl+Shift+C` / `Ctrl+Shift+V` |
| 新会话 / 上一个、下一个会话 | `Ctrl+Shift+T` / `Ctrl+Shift+[`、`]` |
| 分屏 / 关闭 pane / zoom | `Ctrl+Shift+D`、`E` / `W` / `Z` |
| 折叠侧栏 / 跳到 needs-you 并查看记录 | `Ctrl+Shift+B` / `J` |
| 翻页 / 最旧历史 / 回实时屏 | `Shift+PageUp/PageDown` / `Shift+Home` / `Shift+End` |
| 打开允许的 OSC 8 网页链接 | 同格完整 `Ctrl+左键点击` |

回看中裸 PageUp/PageDown 继续本地翻页；备用屏交给程序。窗口快捷键保持先于 Kitty/IME 处理。

## 验证

本机 Arch Linux，Rust stable 1.99.1、GTK 4.24.1，Xvfb X11、Mesa 软件 OpenGL ES 3.2、fcitx5 拼音。
独立 HOME/XDG/socket/X display/D-Bus，真实宿主 PTY；不读取用户 shell rc，不影响用户桌面、输入法或浏览器。

```sh
cargo fmt -p pigeon-linux --check
cargo clippy -p pigeon-linux --all-targets --offline -- -D warnings
cargo test -p pigeon-linux --offline
cargo build -p pigeon-linux -p pigeon-core --offline
python3 tools/checks/linux-modern-input-check.py
python3 tools/checks/linux-input-check.py
PIGEON_SCROLL_IDLE_CHECK=1 python3 tools/checks/linux-scroll-check.py
PIGEON_TEST_NO_SCROLLBACK=1 python3 tools/checks/linux-scroll-check.py
python3 tools/checks/linux-sidebar-check.py
```

先补正例/畸形输入测试；首次编译确认缺失模块。真实键盘回放复现“修饰键先释放导致字符 release 丢失”，
增加失败回归后修复。URI `%ff` 反例发现 GLib 字符串反转义的 UTF-8 前提，改为先字节解码和 UTF-8 校验。
单测 **75 passed、2 个既有 ignored**；fmt/clippy/build 通过。

[现代输入脚本](../tools/checks/linux-modern-input-check.py) 精确检查 DECCKM/DECKPAM、Kitty 查询/消歧、
真实按下/自动重复/释放/修饰键、全键模式中文 preedit 零字节和一次提交、SGR 点击/释放/滚轮/拖动/无键移动、焦点、
历史操作隔离、输入回实时屏的成对释放。它在隔离 HOME 注册 HTTP(S) 测试 handler，实际经过 GIO 调用，
只把 argv 记到文件；验证完整点击一次、普通点击/拖动/更新/恶意目标不打开，无真实网页打开。
既有输入、回看（含 47 秒空闲保活与不协商 scrollback 的分支）和侧栏脚本通过；鼠标模式下的回看脚本断言更新为实际鼠标报告且没有伪造箭头。

## 未验与限制

- 未验真实 Wayland、物理键盘/鼠标/触控板、非美式布局、AltGr、死键、IBus/日文输入法、混合 DPI/分数缩放；
  GDK layout 备用键与双侧修饰键有纯逻辑覆盖，不能视为硬件验收。媒体键等表外功能键不报告。
- 本次用 raw PTY fixture 精确观察协议字节，未运行真实 vim/less 交互；没有长期输入压力/远端延迟或多客户端验收。
  Xvfb 的 DRI3/portal 环境警告不等于硬件 GPU 验收。
- 回看期间宿主暂停实时画面，客户端无法获知应用刚改变的 modes；输入恢复沿用最后收到的 modes，后续快照更新。
  不新增核心模式查询或稳定历史锚点。鼠标仅垂直滚轮，不含水平滚轮、扩展按钮、像素鼠标协议和拖选复制。
- 没有进行 macOS SDK 构建、托管 Ubuntu CI、完整 core 测试或 VT golden；本片不改核心/协议/C ABI/渲染/字体。
  按并行任务范围只更新本文，共享 README、AGENTS、plan/progress 与历史专题状态由父任务整合时同步。
