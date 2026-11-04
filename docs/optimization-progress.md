# 优化执行记录

> 对应 [优化计划](optimization-plan.md)。本页记录实际完成切片和验证，不代表 S0–S7 已整体通过。

## 2026-11-04：第一批实现

### 已落地切片

- S0：加入 Ubuntu GTK 构建、clippy、纯逻辑测试作业；提供 `tools/bench/environment.py` 环境记录。
  宿主测试通过独立 HOME/配置目录、显式 shell/TERM 隔离用户 rc；格式化 Linux 已有代码，最低 Rust 版本对齐 gtk4-rs 的 1.92。
- S1：修复宿主继承外部 TERM。先用真实 PTY 回归复现 `TERM=linux` 误传，再固定 pane 为 xterm-256color/truecolor；
  linux/dumb/xterm-kitty/未设置四种启动环境都覆盖。
- S1：macOS 会话按各 pane 最新状态聚合，idle 不覆盖兄弟 blocked；ack 指向来源 pane；同运行期重播终态不增加未读。
  前台可见的 pane 才自动确认，zoom 隐藏兄弟保留未读；⌘J 定位来源 pane 并解除 zoom；宿主 instance 改变时清除旧连接状态。
- S1/S5：新增安全 `api::delta_of`，与 `pg_msg_delta` 共用验证；FFI 消息编解码也经 api。
  非法行宽、重复/乱序行拒绝；Linux 已协商 delta，验证 seq/网格后原子应用，失败重新 attach，重复帧无害。
  渲染仍整屏重建，尚未实现 GTK 脏行复用。
- S2 首片：`ATTR_WRAP` / `PG_ATTR_WRAP`（64）贯通核心、快照、delta、历史和 C ABI；
  macOS 复制合并自动折行、保留真实换行及折行处空格。旧宿主不发此位时保持原行为。
  `TerminalCell` 抽出为纯数据，选择逻辑能脱离 AppKit 验证。完整字素/组合码点仍未解决。

### 验证

环境：本机 Arch Linux x86_64、Rust 1.99.1、GTK 4.24.1。
Swift 纯逻辑使用临时目录中的官方 Swift 6.1.2 Linux 工具链；没有替换系统工具链或用户配置。

- TERM 回归：修复前实际失败，修复后四种外部环境通过。
- 状态聚合/错误 pane ack/重复未读：新增 Swift 测试先失败，修复后通过；补了隐藏 pane 和移除 pane 的测试。
- 软换行：复制测试先失败，修复后通过；核心测试验证明确换行与自动折行、快照、delta、历史、C ABI。
- `TERM=linux cargo test --locked --offline --quiet`：202 passed、2 ignored（含文档测试）。
- `cargo test --no-default-features --locked --offline --quiet`：143 passed、1 ignored；证明 Rust 客户端无需 internals/host。
- `cargo test -p pigeon-linux --locked --offline --quiet`：20 passed；GTK 客户端构建通过。
- 核心默认/无默认 feature 与 GTK 的 clippy 均以 `-D warnings` 通过，`cargo fmt --all --check` 通过。
- Linux 本机 release 宿主 1,780,192 bytes（约 1.70 MiB），低于 8 MiB；各 musl 架构仍需 release CI 实测。
- VT golden 8/8；cbindgen **0.27.0** 重新生成并比对通过；FFI import 检查、文档链接、`git diff --check` 通过。
- 尚未执行新的 GitHub Actions 作业；本地通过不代表远端 CI 已通过。
- Swift 纯逻辑通过 `tools/checks/swift-logic.sh`：31 tests、0 failures；修改的 AppKit 文件做语法检查，未进行 macOS 类型检查、链接或真实窗口验收。

### 未完成与下一步

1. macOS 完整 `swift build && swift test`、窗口焦点/遮挡/跨主机/zoom 的真实交互验收。
2. 未读事件身份跨应用重启持久化、同 pane 多记录的完整同步；当前仍受宿主只发送最新未确认记录的协议限制。
3. 完整字素、宽度与软换行占位处理（特别是宽字符被推到下一行时的前导空格）、搜索/跨历史选择。
4. GTK IME/剪贴板/回看、渲染脏行与真实桌面验收；CI 配置不等于图形验收。
5. 完整性能基线、16 pane/弱网/功耗测量和 S0 的属性/fuzz 覆盖。

本批未发布、未推送 PR、未改动用户 shell/tmux 配置。

## 2026-11-04：推送前合并远端

fetch 后发现本地分支与 `origin/main` 分叉（本地既有 Linux 4 个提交，远端 53 个提交）。保留本地七个优化/文档提交后，合并远端 `edba2ed`。
保留远端的 Harp 单行 UI、Kitty 键盘协议、编程连字、同步更新、socket 安全和渲染/宿主优化；本地 pane 状态、TERM、软换行、GTK 和 CI 切片叠加其上。
`api::delta_of` 采用远端已公开的 `Delta` 返回类型，追加 modes，保留本地行验证；Linux 与新增测试同步适配。旧评估对应合并前本地版本，当前文档已纠正范围。

合并后本机 Rust 全量：241 passed、2 ignored；后续 GitHub CI 结果追加于此。合并不是“覆盖远端”的强制推送。
