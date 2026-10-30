# pigeon 前端（GUI）路线

> 结论：**内核用 Rust（见 `docs/core.md`），前端按平台写原生**——macOS 用 **AppKit（Swift）**，
> Linux 用 **GTK4**。学 Ghostty/kitty 的做法，不学 Alacritty/WezTerm 的“单套自绘 UI”。
> 理由只有一个：**侧栏是这个产品的主体**，它必须像原生控件（分组、行、悬停、右键、字典序排序、无障碍），
> 而不是在 GPU 画布里自己画出来的假控件。

## 1. 现在流行的终端都怎么做的

| 项目 | 内核 | 前端 / 窗口 | 侧栏/标签这类“chrome” | 观察 |
|---|---|---|---|---|
| kitty | C（+Python 编排） | **按平台**：macOS Cocoa、Linux GLFW（X11/Wayland），OpenGL | 自绘标签栏（C） | 一个内核 + 多个平台前端 |
| Ghostty | Zig（libghostty-vt） | **按平台**：macOS AppKit/Swift、Linux GTK4，Metal | **原生控件** | 与我们要做的事最像 |
| iTerm2 | 自有（Obj-C） | AppKit，macOS only | 原生 + 自绘混合 | 原生体验的上限 |
| Windows Terminal | 自有（C++） | WinUI/XAML | 原生 | — |
| Alacritty | Rust（`vte`+自有 term） | **单套**：winit + OpenGL | 无（只有窗口） | 不做 chrome 才适合单套 |
| WezTerm | Rust（自有 term + mux） | **单套**：winit + wgpu | 自绘标签栏 | 单套能行，但 chrome 只能自绘 |
| Rio | Rust | 单套：winit + wgpu | 自绘 | 同上 |
| Tabby / Hyper / Wave | 各自 | **Electron/Web** | DOM/CSS | UI 最快，终端保真与内存最差 |
| Terminal.app | 自有 | AppKit | 原生 | — |

一句话总结：**要么共用一套自绘 UI（Alacritty/WezTerm/Rio），要么按平台写原生前端（Ghostty/kitty/iTerm2）。**
分界线就是“是否需要一个像样的、原生外观的侧栏”。

## 2. 四个候选

| 方案 | 一套代码跨平台 | macOS 原生感 | 侧栏实现成本 | 终端保真/延迟 |
|---|---|---|---|---|
| **A. 原生分平台：AppKit + GTK4（推荐）** | ❌（两份展示层） | ★★★★★ | 低（用原生控件） | 高（可上 Metal） |
| B. Qt6（Widgets 或 Quick）单套 | ✅ | ★★★ | 中（Qt 控件，macOS 上略“外来”） | 中 |
| C. winit/wgpu 单套自绘（Alacritty 路线） | ✅ | ★★ | 高（行、滚动、悬停、文本编辑全自己画） | 高 |
| D. Electron/Tauri + xterm.js | ✅ | ★★（假原生） | 低（DOM） | 低（JS/VT 保真、内存） |

- A 的唯一代价是“两份展示层”，但**行为逻辑（侧栏模型、布局、协议）都在 Rust 核心里**，
  展示层只做“把数据画成原生控件”，代码量可控（估：侧栏 ~800 行 Swift / ~800 行 GTK）。
- B 的诱惑是省一份 UI，但 Qt 在 macOS 上的控件观感与 AppKit 差距明显——而我们的卖点正是观感。
- C 的问题是：为了一个侧栏要在 GPU 画布上重写滚动、选中、悬停、右键菜单、无障碍、屏幕阅读器、
  拖拽重排，成本高于写两份原生展示层，且做不出系统级细节（例如原生上下文菜单、服务菜单）。
- D 的终端保真与内存代价对“终端应用”是致命的；而且我们要的状态栏/侧栏并不需要 Web 技术。

## 3. 决定

```
Rust core（VT / 布局 / 状态 / 协议）  ── C ABI ──┬─ mac 客户端：Swift + AppKit（M0 首发）
                                                └─ linux 客户端：GTK4（M5）
```

- **macOS 首发用 AppKit**（不是 SwiftUI 全家桶）：侧栏用 `NSOutlineView` + diffable data source，
  行用 `NSTableCellView` 子类；窗口 chrome 用原生标题栏 + 内嵌红绿灯。
  SwiftUI 只用在少数“静态面板”（设置、连接对话框）。
- **终端网格渲染**：M0 用 `CoreText` + 自绘 `NSView`；Metal 渲染器（issue #1）已在
  `PIGEON_RENDERER=metal` 开关后落地：每个可见 pane 一个渲染线程、`FrameMailbox` 短持锁交帧、
  灰度/彩色两张字形图集、三份实例缓冲轮换、按 delta 只重建脏行、隐藏 pane 释放线程与 GPU 资源。
  帧节奏由渲染线程上的 `CAMetalDisplayLink` 驱动（macOS 14+，最高 120Hz）：有新帧才取消暂停，
  信箱空了就暂停，大量输出时每次刷新只画一帧；macOS 13 退回"投递即画"。
  被遮挡的窗口，系统会把 display link 限到约 1Hz，所以遮挡时 Metal 几乎不出帧；CoreText 路径遮挡时仍按 ~60 帧/秒画。
  基准：`PIGEON_FRAME_STATS=1` 每秒打一行帧时间 p50/p99（Metal 另带 GPU 时间），
  `tools/render-bench.sh` 跑同一份大文件 `cat` 加 5 秒持续输出，对比两种渲染器；跑的时候窗口要在最前。
  对齐后删除 CPU 路径；未做：Linux 后端、滚动的行复用。
  对账：`tools/metal-gui-check.sh`（同一会话两种渲染器、两种外观各截一张）。
- **输入法**：实现 `NSTextInputClient`（候选窗定位、marked text）；这是原生前端的额外活，
  但也是 Electron/winit 路线最难对齐的部分，正是选 A 的理由之一。
- **Linux（M5）**：GTK4 + `GtkTextView` 不行（终端要自绘网格），用 `GtkDrawingArea` + Pango 排版；
  侧栏用 `GtkListBox`/`GtkTreeListModel`。行为与 macOS 对齐靠 `docs/ui.md` 的规格。

## 4. 展示层要共享什么

| 放在 Rust core | 放在各平台展示层 |
|---|---|
| 侧栏分组、排序、未读、`needs you` 计数 | 控件、布局约束、动画、悬停/右键菜单 |
| 布局树（嵌套分屏、比例、最小尺寸） | 分隔线拖动的手势与光标 |
| 相对时间的文案与刷新节流 | 本地化格式（日期/时间显示） |
| 快捷键的“动作”语义（下一个 blocked、新建会话） | 键位绑定到菜单/系统快捷键（⌘J 等） |
| 终端屏幕快照/脏行 | 网格绘制、字体选择、连字、光标动画 |

这样两份 UI 只是“把同一份模型画出来”，不会出现“mac 能分组、Linux 不能”的语义差异。

## 5. 验收（前端）

- macOS：侧栏滚动 60fps、分组折叠无闪烁；`⌘J`/点击切换 <100ms；中文输入法候选窗跟随光标；
  VoiceOver 能读出"分组 / 会话名 / 状态 / 未读"。
- 终端网格：16 pane × 1MB/s 输出不掉帧；真彩色/连字/宽字符/备用屏与参考实现一致（M1 起的 `tools/vt-parity`）。
- Linux（M5）：与 `docs/ui.md` 的交互逐条对齐，允许外观不同、不允许行为缺失。
