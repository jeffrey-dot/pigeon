# ADR-0007: 客户端按平台原生，macOS AppKit + Metal 首发

- 状态：已采纳
- 日期：2026-10-27
- 依据：[gui.md](../gui.md)、[ui.md](../ui.md)

> **2026-11-04 实现说明**：保留本 ADR 的原始决策背景。Linux 的 `GtkDrawingArea` 方案已由
> [ADR-0011](0011-linux-rendering.md) 的 `GtkGLArea` + OpenGL 取代；macOS 侧栏实际用 `NSTableView`。
> 下文“分组、排序、快捷键都在核心”是原文中的错误归属：按 `AGENTS.md`，这些 UI 派生逻辑在客户端，
> 核心只负责 VT、布局、状态记录与协议。当前分工见 [gui.md §4](../gui.md#4-核心与客户端的实际分工)。

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
