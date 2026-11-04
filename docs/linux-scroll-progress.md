# Linux 回看切片

2026-11-05，基于 `cc51ce22aec38519ef038b6ac7f2d8791c36ccc2`，对应
[优化计划](optimization-plan.md) S5 的 Linux 回看与输入恢复切片。
本记录与 [Linux README](../clients/linux/README.md) 描述此切片；不代表 S5 或 Linux 功能对齐完成。

## 用户行为

- 协商 `scrollback` 后才接管本地回看；`attach` 仍请求 `history:false`，通过 `scroll` 取得宿主计算的一屏。
  客户端不保存整段历史、不解析 VT、不重算布局。未协商时不显示位置条，翻页键仍按 legacy 编码发送。
- 操作对象为焦点 pane：滚轮每档三行，触控板 surface 位移按逻辑行高累积小数，避免细小位移丢失。
  切 pane、失焦、手势开始、键盘导航、回实时屏时重置累积；Ctrl/Alt/Super/Meta 修饰滚轮不触发回看。
- `Shift+PageUp/PageDown` 每次移动 `rows - 1` 行；回看中裸 PageUp/PageDown 也翻页。
  实时屏的裸 PageUp/PageDown 和 Ctrl/Alt 修饰键交给程序，不占用 shell 的裸 Home/End。
  `Shift+Home` 到最旧历史，`Shift+End` 或 “Return to live” 按钮回实时屏。
- 备用屏禁止回看，所有 PageUp/PageDown 发给程序。仅在 `?1007` 开启且未启用鼠标上报时，将滚轮编码成方向键，遵循 DECCKM。
  鼠标上报、Kitty keyboard protocol 不属于本片；鼠标上报模式下不伪造滚轮方向键。
- 每 pane 显示回看位置条，焦点 pane 显示 `Scrollback · offset / history_len` 和返回按钮。
  数值来自最后采用的宿主回看快照，阅读冻结屏幕时不会随后台输出估算或增长。
- 输入键、IME preedit 开始或有效异步粘贴先请求回实时屏；commit/键/粘贴字节仍只发送一次。
  preedit 期间的翻页键优先交给输入法；滚动取消 preedit，切 pane/失焦继续取消未完成的粘贴与 preedit。
  空剪贴板不改变回看位置。IME 候选窗随回实时屏快照重新定位。
- resize、分屏或 zoom 实际改变网格尺寸时回实时屏，重新取得与新尺寸对应的屏幕。
  单纯点击切 pane 保留各自的回看状态；关闭 pane 后的迟到快照不会把它重新放回输入目标。
- `Ctrl+Shift+C` 复制焦点 pane **当前可见的一屏**，可以是回看屏幕，保留附加码点和软换行。
  未实现拖选、跨屏选择、全历史复制或搜索；位置条只展示位置，不支持拖动。
- 握手后每 15 秒发送 `ping`，防止长时间阅读回看屏幕触发宿主默认 45 秒入站空闲超时。
  这只是连接保活，不包含无 pong 超时检测、断线重连或恢复输入。

## 时序与模块

`scroll.rs` 仅管理每 pane 的已显示位置和未完成请求数。协议的 `scroll` 是相对位移、没有请求 id，
其答复按发送顺序到达，且只有答复带 `history_len`/`offset`。每次动作立即按顺序发出，
多个动作尚未完成时跳过中间答复，只显示最后一屏；不把夹取前的相对位移在客户端合并成一个错误的绝对位置。
连续输入只发一次待完成的回底部请求，之后的新滚动仍能生效。

`screen.rs` 在回看或等待回看答复期间忽略实时 delta 与无回看字段的快照，避免历史屏幕被撕裂。
最后一个回看快照建立新的 delta 序号基准；回底部后恢复连续序号和网格校验，缺序重新 attach。
较旧实时快照不能倒退已恢复的序号；resize 快照可以在同一 seq 更换尺寸。
回看元数据必须成对且 `offset <= history_len`，备用屏 offset 必须为零；快照替换保留进程退出标记。

`scroll_input.rs` 保持纯输入映射和位移累积；`view/scroll.rs` 处理 GTK 滚轮、导航、返回按钮与位置条。
IME/粘贴通过 workspace 的统一输入发送入口回底部，使用弱引用避免引用环。
位置条使用 GTK DrawingArea 浮层，不改字体、字形缓存、整形或 GL 上传路径。
新增 `gtk4` 的 `v4_8` feature 读取滚动单位，因此最低 GTK 为 4.8；没有新增 crate。
所有修改的 Rust 产品文件低于 400 行。

## 验证与证据

环境：Arch Linux，Rust 1.99.1，GTK 4.24.1，gtk4-rs 0.11.5，fcitx5 5.1.23 拼音，
独立 Xvfb X11，Mesa 26.2.6 OpenGL ES 3.2 软件渲染，debug 构建，默认字体，
初始 80×24（640×408 像素），另验 720×450、分屏和 zoom，缩放 1。

先复现纯逻辑失败，再修复：实时 delta 覆盖历史屏幕、不完整回看元数据被接受、迟到实时快照倒退 seq。
新增测试覆盖快速回看/回底部/再回看时序、尺寸变化、退出状态保留、按键归属、小数累积和非法数值。
最终 `cargo test -p pigeon-linux`：**40 passed，1 ignored**（既有手动整形基准）。
`cargo fmt -p pigeon-linux --check`、`cargo clippy -p pigeon-linux --all-targets -- -D warnings`、
`cargo build -p pigeon-linux` 与 `cargo build --bin pigeon-server` 通过。

[回看隔离验收脚本](../tools/checks/linux-scroll-check.py) 使用临时 HOME、XDG 目录、socket、shell 程序、
Xvfb、D-Bus 和 fcitx5，不接触用户桌面或 shell rc。它复用既有输入脚本的隔离生命周期；
[记录代理](../tools/checks/helpers/linux-scroll-proxy.rs) 位于 tools，调用真实 `pigeon_core::api` 编解码，
将回看答复延迟 80ms，保留消息和宿主屏幕首行证据，不在 Python 重写分帧/CBOR。

通过的桌面断言：

- 输出 11,050 行后宿主报告 `history_len=10000`；第一屏回看为 `offset=23`、首行 `LINE-11004`。
  CtrlShiftC 的文本首行与宿主快照相同；后台继续输出时复制文本保持不变。
- `Shift+Home` 两次均钳制到 `offset=10000`，最旧屏幕稳定。
  连续 PageUp/PageUp/PageDown/ShiftEnd 产生相对请求 `-23/-23/+23/0`，最后显示实时屏，后续 delta 可见。
- 连续四次滚轮到 `offset=12`；输入 `x`、异步粘贴、按钮、resize 均回实时屏，程序收到字节准确。
- 回看中拼音 preedit 回实时屏但零字节，确认候选后仅一次“你好”；备用屏 PageUp、ShiftPageDown 原样发送，
  滚轮在 DECCKM 下发送三次 `ESC O A`；关闭 `?1007` 或启用鼠标上报后不发送伪造方向键。
- 分屏中各 pane 独立回看、复制和焦点输入，zoom 改尺寸回实时屏，关闭后焦点恢复。
- 保持历史屏幕 **47 秒**，超过默认 45 秒空闲阈值；记录到 15/30/45 秒 `ping` 与对应 `pong`，
  历史文本不变，之后回实时屏继续收到程序输出。
- 单独移除 hello_ack 的 `scrollback` 能力，客户端不发送 scroll；ShiftPageUp/ShiftEnd 到达程序。
- 没有 GTK critical 或 Rust panic。Xvfb 的 DRI3/portal 环境警告不影响上述断言。

既有 [linux-input-check.py](../tools/checks/linux-input-check.py) 回归全部通过：普通键/Ctrl/Alt/方向键、
fcitx5 中文、括号/普通/空粘贴、组合重音/CJK 复制、分屏/zoom/关闭、切 pane 取消异步粘贴和 preedit、
窗口失焦后恢复。截图检查修正了位置文字对比度，并确认回看位置条与内容可见。

复跑命令：

```sh
cargo build --bin pigeon-server
cargo build -p pigeon-linux
python3 tools/checks/linux-input-check.py
python3 tools/checks/linux-scroll-check.py
PIGEON_SCROLL_IDLE_CHECK=1 PIGEON_SCROLL_ARTIFACTS=/tmp/pigeon-scroll-evidence \
  python3 tools/checks/linux-scroll-check.py
PIGEON_TEST_NO_SCROLLBACK=1 python3 tools/checks/linux-scroll-check.py
```

脚本另需 Xvfb、xdotool、PyGObject、dbus-run-session、fcitx5 拼音和 GTK4 IM module；
保存截图还需 ImageMagick `import`。产物目录保存 `wire.log`、`client.log`、`host.log` 与 `history.png`。
文档相对链接检查与 `git diff --check` 通过。

## 未验与限制

- 真实 Wayland/硬件触控板、惯性滚动手感、IBus、日文 IME、非美式键盘、混合 DPI/分数缩放未验。
  surface 小数累积只有纯逻辑测试；Xvfb 的滚轮测试不能替代真实触控板。
- 多会话选择与宿主 layout 订阅的既有 scope 未改；测试仅一个会话中的多 pane。
  不包含远端连接、多客户端尺寸策略切换或会话恢复验收。
- 回看屏幕冻结时无法实时获知程序进入备用屏；下次 scroll 答复或回实时屏时从宿主更新模式。
  持续输出导致历史淘汰时，后续翻页仍遵循宿主相对最新历史的 offset，不保证历史行身份锚定。
- 连续滚动保留每个请求，直到最后答复才换屏；极长手势/高延迟下的请求合并与队列预算尚未优化，
  没有网络压力、恶意宿主、超大剪贴板或长期 soak 验收。
- 未运行本提交的 macOS 构建/测试或托管 Ubuntu CI，也未复跑完整核心测试/VT 对账；核心语义、协议和 C ABI 均未修改。
  本地 GTK fmt/clippy/测试/构建与上述隔离交互结果不替代跨平台验收。
