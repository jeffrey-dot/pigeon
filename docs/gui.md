# pigeon 前端（GUI）路线

> 结论：**内核用 Rust（见 `docs/core.md`），前端按平台写原生**——macOS 用 **AppKit（Swift）**，
> Linux 用 **GTK4**。学 Ghostty/kitty 的做法，不学 Alacritty/WezTerm 的“单套自绘 UI”。
> 理由只有一个：**侧栏是这个产品的主体**，它必须像原生控件（分组、行、悬停、右键、字典序排序、无障碍），
> 而不是在 GPU 画布里自己画出来的假控件。

> **2026-11-04 实现核对**：macOS 已接 Metal；Linux GTK4/OpenGL 首片已落地。当前范围见 [plan.md §11](plan.md#11-当前能力与已知缺口)。

## 1. 选型时的终端前端对照（非性能排名）

| 项目 | 内核 | 前端 / 窗口 | 侧栏/标签这类“chrome” | 观察 |
|---|---|---|---|---|
| kitty | C（+Python 编排） | **按平台**：macOS Cocoa、Linux GLFW（X11/Wayland），OpenGL | 自绘标签栏（C） | 一个内核 + 多个平台前端 |
| Ghostty | Zig（Ghostty 内核） | **按平台**：macOS AppKit/Swift + Metal、Linux GTK4 + OpenGL | **原生控件** | 与我们要做的事最像 |
| iTerm2 | 自有（Obj-C） | AppKit，macOS only | 原生 + 自绘混合 | 原生体验的上限 |
| Windows Terminal | 自有（C++） | WinUI/XAML | 原生 | — |
| Alacritty | Rust（`vte`+自有 term） | **单套**：winit + OpenGL | 无（只有窗口） | 不做 chrome 才适合单套 |
| WezTerm | Rust（自有 term + mux） | **单套**：自有窗口层 + OpenGL/WebGPU | 自绘标签栏 | 单套能行，但 chrome 只能自绘 |
| Rio | Rust | 单套：winit + wgpu | 自绘 | 同上 |
| Tabby / Hyper / Wave | 各自 | **Electron/Web** | DOM/CSS | Web UI；兼容性和资源占用须独立测量 |
| Terminal.app | 自有 | AppKit | 原生 | — |

一句话总结：**要么共用一套自绘 UI（Alacritty/WezTerm/Rio），要么按平台写原生前端（Ghostty/kitty/iTerm2）。**
分界线就是“是否需要一个像样的、原生外观的侧栏”。

## 2. 四个候选

| 方案 | 一套代码跨平台 | macOS 原生感（选型判断） | 侧栏实现成本 | 渲染路径与验证 |
|---|---|---|---|---|
| **A. 原生分平台：AppKit + GTK4（推荐）** | ❌（两份展示层） | ★★★★★ | 低（用原生控件） | macOS 已上 Metal；延迟需实测 |
| B. Qt6（Widgets 或 Quick）单套 | ✅ | ★★★ | 中（Qt 控件，macOS 上略“外来”） | 依赖 Qt 后端，需实测 |
| C. winit/wgpu 单套自绘（Alacritty 路线） | ✅ | ★★ | 高（行、滚动、悬停、文本编辑全自己画） | GPU 路径，需实测 |
| D. Electron/Tauri + xterm.js | ✅ | ★★（假原生） | 低（DOM） | 取决于 VT 引擎与渲染实现，需实测 |

- A 需要维护两份客户端。VT、布局与协议共用 Rust 核心；侧栏分组、排序、未读与文案属于客户端纯逻辑，macOS 当前由 Swift 实现，Linux 尚需按同一规格补齐。
- B 的诱惑是省一份 UI，但 Qt 在 macOS 上的控件观感与 AppKit 差距明显——而我们的卖点正是观感。
- C 的问题是：为了一个侧栏要在 GPU 画布上重写滚动、选中、悬停、右键菜单、无障碍、屏幕阅读器、
  拖拽重排，成本高于写两份原生展示层，且做不出系统级细节（例如原生上下文菜单、服务菜单）。
- D 引入 Web 运行时；本项目选择平台原生控件。没有同负载测试时，不根据 UI 技术推断终端兼容性或性能高低。

## 3. 决定

```
Rust core（VT / 布局 / 状态 / 协议）
    ├─ C ABI（ffi）→ macOS：Swift + AppKit + Metal
    └─ 安全 Rust API（api）→ Linux：Rust + GTK4 + OpenGL
```

- **macOS 首发用 AppKit**（不是 SwiftUI 全家桶）：侧栏用 `NSTableView` 与自有分组/行模型，
  行用 `NSTableCellView` 子类；透明标题栏内嵌红绿灯。连接与确认对话框走 AppKit。
- **终端网格渲染**：只有 Metal 一条路径（issue #1；M0 的 CoreText + 自绘 `NSView` 已删除，
  CoreText 只留下字体度量与字形光栅化）：每个可见 pane 一个渲染线程、`FrameMailbox` 短持锁交帧、
  灰度/彩色两张字形图集、三份实例缓冲轮换、按 delta 只重建脏行、隐藏 pane 释放线程与 GPU 资源。
  帧节奏由渲染线程上的 `CAMetalDisplayLink` 驱动（macOS 14+，最高 120Hz）：有新帧才取消暂停，
  信箱空了就暂停，大量输出时每次刷新只画一帧；macOS 13 退回"投递即画"。
  被遮挡的窗口，系统会把 display link 限到约 1Hz，所以遮挡时几乎不出帧。
  基准：`PIGEON_FRAME_STATS=1` 每秒打一行帧时间 p50/p99（含 GPU 时间），
  `tools/bench/render-bench.sh` 跑同一份大文件 `cat` 加 5 秒持续输出；跑的时候窗口要在最前。
  删除前在前台窗口实测（2560×1664 60Hz，10 万行 cat + 5 秒持续输出）：
  CoreText 帧 CPU p50 3.6ms / p99 12.4ms，App CPU 3.36s；Metal 帧 CPU p50 1.6ms / p99 1.8ms
  （GPU p50 0.3–0.5ms，偶发 p99 到 15ms），App CPU 2.68s；两者都跟满 60 帧/秒。
  未做：滚动的行复用、DEC mode 2033 可见性通知；Linux 已有 OpenGL，后台渲染线程与 DMABUF 尚未接入。
  对账：`tools/checks/metal-gui-check.sh`（两种外观各截一张）。
- **输入法**：实现 `NSTextInputClient`（候选窗定位、marked text）；这是原生前端的额外活，
  但也是 Electron/winit 路线最难对齐的部分，正是选 A 的理由之一。
- **Linux（M5）**：GTK4 + `GtkTextView` 不行（终端要自绘网格）；网格已用 `GtkGLArea` + OpenGL ES 3 / 3.3 core
  （实例化四边形 + R8/RGBA 字形图集，字体走 `fontdb` + `swash`，不用 Pango 排版）；
  侧栏计划使用 GTK 原生列表，目前未实现。行为与 macOS 对齐靠 `docs/ui.md` 的规格。
  渲染形状与参照实现见 [renderer.md](renderer.md)，选型理由见 [adr/0011](adr/0011-linux-rendering.md)。

## 4. 核心与客户端的实际分工

| Rust core / 宿主 | 各平台客户端 |
|---|---|
| OSC 7501 解析、记录生命周期、状态消息 | 侧栏分组/排序、未读、needs-you、通知与文案 |
| 布局树、比例、最小尺寸、焦点与 zoom | 分隔线手势、菜单、快捷键映射 |
| `age_ms` 状态时长 | 本地时间换算、相对时间显示与刷新 |
| 屏幕快照/脏行、VT 模式 | 字形/字体、绘制、IME、选择与剪贴板 |

macOS 的 `SidebarModel` / `SessionReducer` / `Grouping` / `KeyEncoder` 是客户端纯逻辑。
Linux 复用核心语义，通过同一 UI 规格和对应测试实现行为对齐；目前尚未对齐，不宣称两份 UI 已共享所有行为。

## 5. 验收目标（未全部完成）

- macOS：侧栏滚动 60fps、侧栏收起/展开无闪烁；`⌘J`/点击切换 <100ms；中文输入法候选窗跟随光标；
  VoiceOver 能读出"分组 / 会话名 / 状态 / 未读"。
- 终端网格：16 pane × 1MB/s 输出不掉帧；真彩色/连字/宽字符/备用屏与参考实现一致（M1 起的 `tools/vt-parity`）。
- Linux（M5）：与 `docs/ui.md` 的交互逐条对齐，允许外观不同、不允许行为缺失。

当前 VT 对账仅有 8 条文本 golden，不验证连字、GPU 像素、IME 或 VoiceOver；16 pane 与端到端输入延迟也需要专门实测。
