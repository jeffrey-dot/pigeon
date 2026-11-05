# 可见网格选择与复制

2026-11-05 核对。核心新增 `api::select_grid`：读取不可变 Snapshot 的当前 screen，返回独立拥有的 UTF-8 文本和每行半开高亮范围。
它不读取隐藏历史，也不解析 PTY 字节。跨历史锚点与淘汰检测继续属于 [文本查询](text-query.md)，本片不宣称完整选择体系完成。

## 共享语义与接口

- `GridSelection` 含 inclusive anchor/head（row/col）、Cell/Word/Line 单位及 rectangle。反向拖动归一到阅读顺序。
- 宽字符续格会扩到其主格，文本只复制一次；附加码点保留。选词包括字母数字和常见路径/URL 标点，当前仅在所在网格行内扩展。
- 线性选择按 `ATTR_WRAP` 合并软折行，保留硬换行并裁掉每段硬行尾 ASCII 空格。矩形选择每行独立、保留所选空格，边缘触及宽字符时扩为完整字符。
- 网格至多 1,000,000 格，单格附加文本最多 4096 字节；返回文本最多 8 MiB。超过预算返回错误，不返回截断的成功副本；这是导出预算，不是进程内存上限。
- Rust：`GridPoint` / `GridSelection` / `SelectionUnit` / `SelectionSpan` / `Selection` 和 `select_grid`。
- C：`PgGridSelection` 带 size/version；`pg_select_grid` → owned `PgSelection`；`pg_selection_text` 借用到该 handle 释放，`pg_selection_spans` 读取范围，`pg_selection_free` 释放。
  源 snapshot/core 释放后选择仍有效。FFI 只转换指针/格子与调用 API；语义在 `core/src/term/selection.rs`。
- 只新增接口，不更改 PgCell 步长或旧消息字段；头文件由 cbindgen 0.27.0 生成。C 输入须为当前库完整 PgSnapshot，旧 wire 仍由现有 API 转为新结构。

## Linux 操作

- 左键拖选；双击选词；三击选行；Alt+拖动矩形选择；应用启用鼠标上报时用 Shift+拖动进行本地选择。
- `Ctrl+Shift+C` 有选区时复制选区，无选区时保留复制当前可见屏幕；`Ctrl+Shift+A` 选择当前可见屏幕，不是全部 scrollback。
- 核心决定文本与高亮范围，GTK 只保存鼠标端点和绘制。当前可见的回看/搜索画面也可选择；任何替换其来源网格的更新或输入会清除过期选区。
- 分隔线和 Ctrl 链接手势优先处理；选区拖动不会向应用发送孤立 mouse release。
- 尚未实现跨屏自动滚动选择、持久历史锚点、键盘扩展选择；拖出当前 pane 时不会选中另一个 pane。

## macOS 接入

macOS 的 `TerminalSelection` 仅保存手势端点/单位，`SelectionGrid` 将当前 Swift-owned 格子和附加文本借用为 PgSnapshot，调用同一 `pg_select_grid`，在释放 C handle 前复制文本与高亮范围。
普通拖选、双击词、三击行继续可用，Option+拖动选择矩形；⌘C 复制选区，⌘A 仅选择当前可见屏幕。
鼠标命中先夹到可见格子，核心处理字宽、词边界与换行。源 snapshot 或 delta 行更新会清空旧选区，避免同一坐标悄悄选择新文字。

每次手势端点变化最多计算一次选择并缓存文本/span；光标闪烁、Metal 重绘、菜单验证、复制读取缓存，不重复转换整屏。
手势计算仍需把可见网格转为 C 外形，并由 FFI 构造安全快照；没有宣称零拷贝或通过超大网格拖动性能验收。
桥接限制与核心一致，非法尺寸/过长文本不产生部分成功副本。此迁移没有改 wire 或生成头文件。

## 验证

Rust 5 条专题检查宽字符/附加码点、双方向、硬软换行、词/行/矩形、畸形网格和预算拒绝；C ABI 2 条检查 owned 生命周期、非法单位/size/version、范围读取。
本机默认 core 334 passed / 2 ignored，无默认 227 passed / 1 ignored；GTK 98 passed / 2 ignored。

```sh
cargo test --test selection --test abi --locked
cargo test --no-default-features --locked
cargo test -p pigeon-linux --locked
python3 -B tools/checks/linux-selection-check.py
tools/checks/swift-selection.sh
```

真实 Xvfb/PTY 检查拖选、反向、CJK/组合文本、双/三击、矩形、Select All、更新清除与 Shift 覆盖应用鼠标。
现代输入、回看、搜索、侧栏与分隔线脚本通过。真实 Wayland/不同双击设置、硬件多 DPI 与跨历史操作尚未验收。

macOS 迁移本机验证：真实 Swift 6.1.2 源码链接核心 C ABI 的 13 项测试通过，覆盖原选择行为、续格端点、矩形、临时存储释放、非法尺寸/超长文本。
侧栏/链接等 Foundation 回归 93 项通过，搜索/ABI 7 项、状态/链接 ABI 3 项通过；选择测试从不链接 Rust 的 swift-logic.sh 移至 swift-selection.sh，不能把两类验证混写。
新增 AppKit 用例检查 Option 手势、缓存和 delta 清除，Linux 仅做语法解析；完整 AppKit 构建/测试等待 macOS CI，真实 GUI 拖动与剪贴板仍待平台验收。
