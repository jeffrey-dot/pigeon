# ADR-0007: 客户端按平台原生，macOS AppKit + Metal 首发

- 状态：已采纳
- 日期：2026-10-27
- 依据：[gui.md](../gui.md)、[ui.md](../ui.md)

## 决定

客户端按平台写原生：macOS 用 AppKit（侧栏 `NSOutlineView` + diffable data source），
Linux 用 GTK4 + `GtkDrawingArea`。SwiftUI 只用于少数静态面板。

终端网格只有 Metal 一条渲染路径：每个可见 pane 一个渲染线程、`FrameMailbox` 短持锁交帧、
灰度/彩色两张字形图集、三份实例缓冲轮换、按 `delta` 只重建脏行。
CoreText 只做字体度量与字形光栅化。

## 约束

- `PigeonCore` 不 `import AppKit`；`import CoreText` 只许在 `CoreTextRenderer.swift`。
- 网格渲染只走 `PaneView+Metal.swift`。
- 分组、排序、布局、状态、快捷键语义都在核心里，展示层只负责画。
- 客户端不碰 PTY，不解析字节流。

## 何时重开

不重开。
