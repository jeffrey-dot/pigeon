# ADR-0011: Linux 客户端第一版：GTK4 + GtkGLArea(OpenGL)

- 状态：已采纳
- 日期：2026-11-02
- 依据：[gui.md](../gui.md)、[renderer.md](../renderer.md)、[protocol.md](../protocol.md)、ADR-0007

## 决定

1. Linux 客户端用 **Rust + `gtk4-rs`**，以 `default-features = false` 依赖 `pigeon-core`，
   只走 `pigeon_core::api`（不碰 `term`/`proto` 内部类型，不自己实现分帧与 CBOR）。
2. 网格渲染走 **`GtkGLArea` + 实例化四边形**：背景一批、字形一批（R8 字形图集）、光标同批。
   shader 以 GLSL 字符串随二进制发布（与 macOS 运行时编译 MSL 同因：不用外部构建规则）。
   版本**不写死**：`set_required_version(3, 0)`，shader 头在运行期按 `glGetString(GL_VERSION)` 选
   `#version 300 es`（GDK 通过 EGL 在 Wayland 给的正是 OpenGL ES 3.2）或 `#version 330 core`（GLX 桌面 GL）。
3. 字体用 **`fontdb` + `swash`**：终端要的是逐格字形，不需要 Pango 的富文本排版。
4. GL 函数用 `libloading` 加载，顺序 `eglGetProcAddress` → `libGL.so.1` → `libEGL.so.1`；
   GDK 在 Wayland 走 EGL、X11 可能走 GLX，两条路径都要能拿到符号。
   字体发现之后按固定名单挂 **fallback 面**（CJK/box-drawing/符号），度量只取主面。
5. 共享渲染层放在**客户端层**（`clients/shared/` 之类，本 ADR 不预设目录）：`core/` 不进任何 GUI 依赖。
   第一版只有一个后端，因此先把这个接口留在 `clients/linux/src/render/` 内部。

## 理由

- 形状对齐 Ghostty（上游 `042b16b`）：一份绘制实现 + 按平台编译的后端，见
  [renderer.md](../renderer.md) §2。它的 OpenGL 后端与我们的需求几乎一一对应。
- `GtkDrawingArea` + Cairo 太重（每格一次绘制调用），`GtkTextView` 不行（终端要自绘网格），
  `winit`/`egui` 自绘会牺牲原生侧栏（[gui.md](../gui.md) §4-6）。
- 不做 DMABUF / 后台渲染线程：那是性能上限，不是第一版的门槛；先把"能连上、能画、能输入"跑通，
  用帧时间数据决定要不要换（[renderer.md](../renderer.md) §5.4、§6）。

## 约束

- `unsafe` 只允许出现在 `clients/linux/src/render/gl.rs`（dlopen + GL 入口点 + `from_raw_parts`
  上传实例缓冲），与 `core/src/ffi/` 同级处理：边界之外全是安全代码。
- Linux 客户端不 import `PigeonApp`、不进 `core/`、不自己解析字节流（AGENTS.md §3）。
- 屏幕来源只有 `snapshot`/`delta`；不做屏幕启发式，不从字节流重放历史（AGENTS.md §7.2）。
- 单元格颜色语义与 macOS 一致：`0` = 主题默认、`0x01000000 | idx` = 调色板、否则 `0xRRGGBB`；
  `attrs` 位含义固定（protocol §3.3）。
- 不可信文本只当纯文本画，不进任何富文本路径（AGENTS.md §7.5）。

## 何时重开

- 当 Linux 上整屏重建的 CPU 帧时间在 16 pane × 大输出的基准里超出预算（`tools/bench/`），
  就重开"后台渲染线程 + DMABUF 导出"（蓝本：Ghostty `src/apprt/gtk/class/render_surface.zig`）。
- 当第二个客户端也要同一份绘制逻辑时，重开"共享渲染 crate 的位置"，仍不放进 `core/`。
