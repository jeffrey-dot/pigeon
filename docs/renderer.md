# 渲染层：参照 Ghostty，落在客户端

> 这份文档回答三个问题：**终端网格怎么画**、**哪部分该共享、哪部分不该**、**Linux 客户端第一版怎么画**。
> 分层理由在 [adr/0007-platform-native-clients.md](adr/0007-platform-native-clients.md) 与
> [adr/0011-linux-rendering.md](adr/0011-linux-rendering.md)；前端路线在 [gui.md](gui.md)。
>
> 上游参考：Ghostty `main` @ `042b16b`（2026-11-01），下文 `file:line` 都是这棵树里的真实位置。

## 1. 一句话结论

Ghostty **共享绘制逻辑，不共享渲染器**：一份 `GenericRenderer`，在**编译期**参数化成 Metal 或
OpenGL 两个后端，每个平台的产物里只有一个后端、只链接那一个后端的系统库。

pigeon 不把这个共享层放进 `core/`（理由见 §4），而是放进**客户端层**：核心只给“这一帧该画什么”的数据，
每个平台各自把数据变成 GPU 指令。

## 2. Ghostty 的分层（实测）

```
┌────────────────────────────────────────────────────────────────────┐
│ 平台外壳                                                            │
│   macOS: Swift/AppKit  → libghostty C ABI（自己一行网格都不画）      │
│   Linux: GTK4          → 同语言 @import，编进同一个二进制            │
├────────────────────────────────────────────────────────────────────┤
│ apprt（应用运行时抽象）  src/apprt.zig                              │
├────────────────────────────────────────────────────────────────────┤
│ 核心  src/App.zig  src/Surface.zig  src/terminal/  src/font/        │
│       src/renderer/generic.zig  ← 3693 行，真正的绘制逻辑            │
│       src/renderer/{Metal,OpenGL}.zig + 各自子目录 ← 资源薄封装      │
└────────────────────────────────────────────────────────────────────┘
```

### 2.1 编译期二选一，没有运行期分发

`src/renderer.zig:38-44`：

```zig
pub const Renderer = GenericRenderer(GraphicsAPI);

const GraphicsAPI = switch (build_config.renderer) {
    .metal => Metal,
    .opengl => OpenGL,
};
```

后端默认值在 `src/renderer/backend.zig`：Darwin → `.metal`，其余 → `.opengl`。
`src/build/SharedDeps.zig:578` 只在 `renderer == .opengl` 时 `linkFramework("OpenGL")`——
所以 **macOS 产物里没有 GTK/EGL，Linux 产物里没有 Metal**。这是它能"共享渲染"却不背两个平台
GUI 依赖的**全部原因**。

代价（值得记住）：跨后端的分支不能写 `if (is_metal)`，只能写成泛型函数被实例化两次；
同一个 bug 要修两遍。

### 2.2 共享层的契约

`src/renderer/generic.zig:60-95` 把后端缩到只剩资源原语：

```
GraphicsAPI → Target → Frame → RenderPass → Step → Pipeline
              Buffer / Texture / Sampler
```

后端必须提供：`Target`（可以是 surface，也可以是离屏 FBO）、`Frame`、`RenderPass`、`Pipeline`、
`Buffer`、`Texture`、`Sampler`、`swap_chain_count`，以及可选的钩子：

| 钩子 | 谁实现 | 语义 |
|---|---|---|
| `loopEnter` | Metal（`Metal.zig:148`） | 把渲染挂到**事件循环**：注册 layer 的 display callback |
| `threadEnter` | OpenGL（`OpenGL.zig:201`） | 把渲染挂到**线程**：GL context 必须在绘制线程上 current |
| `drawFrameStart` / `drawFrameEnd` | 都实现 | 每帧的前后包裹（Metal 里是 autorelease pool） |

所有**真的绘制决定**都在共享层：pass 顺序（背景 → 文字 → 图片 → 光标 → 自定义 shader，
`generic.zig:1933/1942/1960/1976/2029` 五段 `pass.step`）、实例布局、uniform、字形图集、脏行/delta、
对比度下限。后端不认识"单元格"这个概念。

体量对比（`wc -l`）：

| | 文件 | 行数 |
|---|---|---|
| 共享 | `generic.zig` + `cell/row/cursor/image/link/Overlay/shadertoy/size/State/Thread` | ≈11k |
| Metal | `Metal.zig` 387 + `metal/*`（11 个） | ≈1.8k |
| OpenGL | `OpenGL.zig` 449 + `opengl/*`（10 个） | ≈1.2k |

### 2.3 平台差异只剩三处

1. **帧节奏**：`src/renderer/DisplayLink.zig` 只有 macOS 有；`generic.zig:42-46` 用编译期
   `switch`，其他平台补成 `void`，相关代码路径直接消失（不是 `if`，是不存在）。
2. **macOS 的呈现**：`metal/IOSurfaceLayer.zig`——自己写的 `CALayer` 子类 + IOSurface 交换链
   （不是 `CAMetalLayer`），换来"什么时候显示"的控制权与遮挡时干净释放。
3. **Linux 的呈现**：`opengl/Target.zig:148 exportDmabuf` → `renderer/Dmabuf.zig`；GTK 侧是自写的
   `GtkWidget`（`src/apprt/gtk/class/render_surface.zig`），主线程按自己的节奏消费 DMABUF。
   文件头注释直说放弃 `GtkGLArea` 的原因：GTK 的渲染生命周期"lives exclusively on the main UI
   thread"，与渲染线程模型冲突。

### 2.4 帧的实际内容

两个 struct 承载几乎所有信息（`metal/shaders.zig`）：每帧一份 `Uniforms`（投影矩阵、屏幕/单元格/
网格尺寸、padding、最小对比度、光标位置与颜色、背景色、若干 bool），每个字形一份 32 字节
`CellText`（图集坐标、bearing、网格位置、颜色、图集编号、若干标志位）。

字形来自共享的 `src/font/` 图集：普通文字一张灰度图集，彩色字形（emoji）一张彩色图集。
渲染线程把终端状态**拷贝**成快照再画（`renderer/State.zig`），不在 GPU 工作期间持终端锁。

## 3. 可以借鉴的、不必抄的

| Ghostty 的做法 | pigeon 的取舍 |
|---|---|
| 编译期选后端，一个产物一个后端 | **抄**。客户端层同样按平台编译，macOS 只编 Metal 路径 |
| 泛型 `Renderer(GraphicsAPI)` 把绘制逻辑与资源原语分开 | **抄**（Linux 版先按最小实现，接口留出第二个后端的形状） |
| 渲染线程 + 邮箱交帧，主线程只转发尺寸/事件 | **抄**。macOS 已这样（`PaneView+Metal.swift` + `FrameMailbox`） |
| 灰度图集 + 彩色图集分开 | **抄**。macOS 与 Linux 当前都已有灰度和彩色两张图集 |
| 脏行增量重建 | macOS 已做；Linux 第一版先整屏重建（见 §5.4） |
| DMABUF 导出 + 自定义 widget | **暂不**。iOSurface/DMABUF 是性能上限，不是第一版的门槛 |
| 运行时编译 MSL / GLSL 字符串 | macOS 已这样（SwiftPM 编不了 `.metal`）；Linux 同理用 GLSL 字符串 |
| 自定义 shader（Shadertoy） | 不做（不在 M5 范围） |

**许可证**：Ghostty 是 MIT。可以借鉴结构与规格，**不逐行复制**；若复制片段必须在文件头注明来源与
许可证（[AGENTS.md](../AGENTS.md) §9）。

## 4. 为什么共享层不进 `core/`

一句话：`pigeon-server` 要走 ssh 落到别的机器上，产物是 **3–8 MB 静态单文件**（[remote.md](remote.md) §27）。

- GUI、字体与图形依赖会扩大宿主依赖面和部署要求，违背无头、体积受控的目标；
- `core/` 要让同一份源码分别在 macOS/Linux 构建，不是共用同一份二进制；平台专属渲染留在客户端，避免把 GUI 的 `cfg(target_os)` 分支引入核心；
- 客户端接口服务于 Swift/C 与 Rust 两种调用方，传递屏幕数据即可，不需要跨语言暴露 GPU 资源；
- 无头环境要能跑 `core/tests/host.rs`、`tools/vt-parity/`，GUI 依赖会把它们变成需要显示器的测试。

结论：共享到**模型**为止。核心给 `Snapshot`/`Delta`/`layout`/颜色语义/`MODE_*` 位；渲染是客户端的事。
这也是 macOS 客户端今天的形状（`GridFrame` → `MetalGridRenderer`），Linux 版照同一条缝切。

## 5. Linux 第一版（M5 的第一个切片）

### 5.1 技术栈

| 项 | 选择 | 理由 |
|---|---|---|
| 语言/绑定 | Rust + `gtk4-rs` | 与 `core/` 同语言，直接 `use pigeon_core::api`，不需要 FFI 胶水；本机 GTK4 4.24 |
| 依赖 | `pigeon-core`（`default-features = false`） | 只看得到 `api`，看不到 `term`/`proto` 内部类型（[AGENTS.md](../AGENTS.md) §3） |
| 侧栏 | GTK 原生控件（本轮不做） | 侧栏是主体，必须像原生控件（[gui.md](gui.md) §4-6） |
| 网格 | `GtkGLArea` + 实例化四边形 | 形状对齐 Ghostty 的 OpenGL 后端与 macOS 的实例化四边形 |
| 字形 | `fontdb` 找字体 + `swash` 光栅化 + 静态 fallback 链 | 纯 Rust，不用 Pango（终端要的是逐格字形，不是富文本）；CJK 靠 fallback 面 |
| GL 函数加载 | `libloading`：`eglGetProcAddress` → `libGL.so.1` → `libEGL.so.1` | GDK 在 Wayland 用 EGL、在 X11 可能用 GLX，两条都要能拿到符号 |

**实测（Asahi + NVIDIA eGPU、niri/Wayland）**：GDK 通过 EGL 给的是 **OpenGL ES 3.2** 上下文，不是桌面 GL。
所以 `GtkGLArea.set_required_version(3, 0)`（不是 3.3 —— 要 3.3 会让话框直接 `Unable to create a
GL context`），shader 版本头在运行期按 `glGetString(GL_VERSION)` 选：含 `OpenGL ES` 用
`#version 300 es` + `precision highp float;`，否则用 `#version 330 core`。
两条都是 `gl_VertexID` + `vertex_attrib_divisor` 支持的版本（ES 3.0 / GL 3.3 core 起）。

### 5.2 形状

```
clients/linux/src/
├── main.rs        装配：GtkApplication → 窗口 → 消息泵
├── host.rs        默认 socket 路径（问 `pigeon-server --socket-path`）、拉起、连接、hello/attach、收发线程
├── screen.rs      一个 pane 的屏幕模型：snapshot 建立 seq，delta 校验后更新；缺序/尺寸不符重新 attach
├── view.rs        Workspace：**一个 GLArea 画所有 pane**、焦点、快捷键、每 pane 的 cols/rows
├── input.rs       按键 → 字节（xterm legacy 编码，含 DECCKM）
└── render/
    ├── mod.rs     Renderer：一帧 = 清屏 + 背景批 + 字形批 + 彩色字形批
    ├── gl.rs      加载器、两套 shader 头（GLSL ES / 330）、pipeline、实例缓冲
    ├── atlas.rs   R8 掩码图集 + RGBA 彩色图集（swash 光栅化、架子装箱、上传）
    ├── font.rs    字体发现、fallback 链（含 emoji）、单元格度量
    ├── boxdraw.rs 制表符（U+2500–257F）与方块元素（U+2580–2593）→ 矩形
    ├── instances.rs 屏幕 → 实例（背景/制表符/下划线/删除线/光标 + 两组字形）
    └── palette.rs 颜色语义：`0x01000000|idx`、xterm 256、默认色、反色
```

### 5.2.1 多 pane：一个 surface，N 个矩形

分屏不按 pane 开 GL 上下文，而是**一个 `GtkGLArea` 画整个窗口**：

- 位置来自宿主的 `layout`（归一化 rect），`view::place()` 把它换成设备像素——纯函数，有单测；
- 每一帧按 pane 顺序：设 scissor 到该 pane 的 rect → 画它的矩形批/文字批/彩色批。
  scissor 是必要的：`cols = floor(w / cell_w)` 只能保证格子不超宽，最后一行仍可能高出几像素；
- 光标只画焦点 pane 的；焦点来自 `layout.focus`，点击做命中测试（rect 包含），并把焦点告诉宿主
  （`focus` 消息 → 尺寸策略 `latest` 跟着走）；
- 分割线用 `layout_splits` 的 rect 画一条 1px 半透明线，否则两个同底色的 pane 看起来像一个宽的；
- zoom：`layout.zoom` 有值时只画那个 pane，铺满窗口。

为什么不是每个 pane 一个 GLArea（macOS 的做法）：GTK 里每个 GLArea 都要自己的 context/FBO 与绘制
调度，而这一侧的布局本来就由宿主给、命中测试只是矩形包含。上下文少一个，帧里就少一次 clear 与
一次上下文切换；代价是 `PaneFrame` 这层抽象与 CPU 侧的索引。

### 5.3 一帧的三组四边形

和 macOS/Ghostty 一样是**实例化四边形**，按内容分为三组 draw：

1. **矩形批**：格背景（改成"同行同色合并"是后续优化）、下划线/删除线、光标
   （方块/下划线/竖线按 `cursor.shape`），以及**制表符与方块元素**——
   它们由 `boxdraw.rs` 直接从单元格几何算出来，不走字体；
2. **文字批**：每格一个四边形，采样 R8 掩码图集的一个矩形，颜色走顶点属性
   （`ATTR_DIM` 用 alpha 0.6，与 macOS 一致）；
3. **彩色字形批**：emoji 采样 RGBA 图集，**预乘 alpha**（`ONE, ONE_MINUS_SRC_ALPHA`），
   文字用直通 alpha（`SRC_ALPHA, ONE_MINUS_SRC_ALPHA`）——同一个 shader，`u_kind` 选采样器与公式。

shader 用 `gl_VertexID` 生成单位四边形（6 顶点，`draw_arrays_instanced`）；矩形实例 32 字节、字形实例 48 字节。

宽字符占两格：字形比两格窄时**居中**而不是拉伸（fallback 面的全角字形常常偏窄）。
现在附加码点通过 cell text 保留；macOS CoreText / Linux swash 以完整 cell 文本整形。
`e + U+0301` 的快照/增量/复制链路已补，跨宽 cell 的 ZWJ/肤色/旗帜整簇宽度仍待完成，不能宣称完整字素支持。

### 5.4 第一版明确不做（与代码现状同步）

- 渲染脏行增量（传输层已接 delta；渲染每帧整屏重建；`FrameStats` 先量出真实数字再决定是否值得做）；
- 滚动回看（`scroll` 消息）与拖选/选择复制（当前屏幕复制已接）；
- 侧栏；
- 拖动分割线改比例（只画线，`resize_split` 还没接）；
- kitty keyboard protocol（先用 xterm legacy 编码）；
- 鼠标上报、焦点事件（GtkIMMulticontext、异步粘贴与宿主括号包装已接）；
- DMABUF / 后台渲染线程（先主线程 GLArea，等有性能数字再换）。

当前支持：制表符/方块元素改由矩形画（`boxdraw.rs`，从 macOS 的
`BoxDrawing.swift` 移植，同一张权重表）、宽字符字形居中、emoji 彩色图集（COLR + CBDT 位图）。

分屏已接：`Ctrl+Shift+D` 左右分、`Ctrl+Shift+E` 上下分、`Ctrl+Shift+W` 关焦点 pane、
`Ctrl+Shift+Z` 放大/还原。左右分屏目视确认过（两个 fish 各自独立、分割线可见）；上下分屏与 zoom
只做了单测级验证（见 §5.5 的说明）。

### 5.5 验收（第一片已过）

以下是第一片当时的实跑记录，保留原始数量与环境；不代表 2026-11-04 对当前版本的重新验收。

在 Asahi + NVIDIA eGPU、niri/Wayland、GTK 4.24 上实跑：

1. ✅ `cargo build -p pigeon-linux`、`cargo clippy -p pigeon-linux -- -D warnings`、`cargo test -p pigeon-linux`（14 个单测）；
2. ✅ 自动拉起本机 `pigeon-server`（`--detach`）→ 握手 → `attach` → 画网格；用 `wtype` 打字验证
   输入 → 宿主 → 回显全链路（`echo hello from pigeon` 正常执行并显示）；
3. ✅ fish 的语法高亮（绿 `~`、青分支、粗体 `help`）与 CJK（`中文`）、全角（`ＡＢＣ`）、预组重音字符（`é ñ`）都能显示
   —— 靠 `fontdb` 的静态 fallback 链（`Noto Sans Mono CJK SC` 等）；
4. ✅ 缩放窗口：`stty size` 打印 `58 140`，与窗口像素 / 单元格尺寸一致（PTY 收到了新尺寸）；
5. ✅ 制表符：`printf "┌───┐\n│abc│\n└───┘"` 连成一体的方框（横线不接缝）、`█▓▒░` 四档亮度正确；
6. ✅ 彩色 emoji：`😀 🚀 ❤️ 👍🏽 🇨🇳` 走 RGBA 图集正常上色（本机只有 `Twemoji`，没有 Noto Color Emoji）；
7. ✅ `vim`：备用屏能进能出（`:q!` 后回到 shell 原屏）、`~` 填充列、状态行、文本光标；
8. ✅ `cargo test --manifest-path core/Cargo.toml` 在 Linux 上 45 个用例全绿（不变量 7）。

9. ✅ 分屏：`Ctrl+Shift+D` 后两个 pane 各自跑 fish、各自尺寸、分割线可见（目视），
   `view::place()` 的几何（半屏/zoom/未知 pane 跳过）有 4 个单测。

未验证（需要手动 / 后续）：上下分屏与 zoom 的目视效果、鼠标上报、IME、`Ctrl-D` 后重连复原、
多客户端尺寸策略标注。

## 6. 后续（M5 剩余 → M6）

1. 脏行增量 + 行复用（对齐 macOS 的 `DirtyRows`）；
2. 侧栏（`GtkListBox`/`GtkTreeListModel`）+ 分组 + 未读/`needs you`；
3. 拖动分隔线与多会话选择（分屏、zoom、布局绘制和焦点命中已接）；
4. 滚动回看、选择、剪贴板（`OSC 52` 只写）；
5. IME（`GtkIMContext`）与 kitty keyboard protocol；
6. 若 CPU 帧时间成为瓶颈：后台渲染线程 + DMABUF 导出（Ghostty 的 `render_surface.zig` 是蓝本）。
