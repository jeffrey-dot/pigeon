# pigeon

一个原生终端：自带一个**隐形的后台会话进程**（关窗口任务继续跑，像 tmux，但不需要你配置任何服务），
以及一等公民侧栏（agent inbox）。
**当前状态（2026-11-04 核对）**：Rust 内核与 `pigeon-server` 支持 macOS/Linux；macOS 客户端已有多 pane、
侧栏状态、SSH 远端会话、Metal 渲染、增量续传、回看、选择复制粘贴、鼠标与输入法。
Linux GTK4 客户端已能连接本地宿主、渲染、分屏、增量更新、IME、滚动回看、复制当前屏幕与异步粘贴，M5 功能对齐仍在进行。
本轮实现与验证见 [优化执行记录](docs/optimization-progress.md)。

会话保存在宿主内存中：关闭客户端后任务继续运行；宿主退出或机器重启后不恢复。
搜索、OSC 8 链接、OSC 52 剪贴板、完整字素等仍待实现；macOS 已接 Kitty keyboard protocol 与编程连字，完整现状见
[当前能力与缺口](docs/plan.md#11-当前能力与已知缺口)。

- [AGENTS.md](AGENTS.md) —— 给编码 agent 的工作说明：**模块划分**、风格、测试、不变量
- [GLOSSARY.md](GLOSSARY.md) —— 领域词汇：一个概念只有一个名字，含「禁用的说法」
- [docs/adr/](docs/adr/README.md) —— **架构决策记录**：为什么是 Rust 内核、为什么宿主持有 PTY、否掉过哪些选项
- [docs/m0-checklist.md](docs/m0-checklist.md) —— **M0 验收记录**：自动化结果、截图证据、未验项
- [docs/m1-checklist.md](docs/m1-checklist.md) —— **M1 验收记录**：分屏、拖动分隔线、关窗重开
- [docs/m2-checklist.md](docs/m2-checklist.md) —— **M2 历史验收**：回看、zoom、多客户端尺寸与恢复
- [docs/m4-checklist.md](docs/m4-checklist.md) —— **M4 历史验收**：SSH 安装、三档对接与断线恢复
- [clients/linux/README.md](clients/linux/README.md) —— **Linux 构建、快捷键与未完成项**
- [docs/renderer.md](docs/renderer.md) —— **渲染实现**：macOS Metal 与 Linux OpenGL
- [docs/m3-checklist.md](docs/m3-checklist.md) —— **M3 验收记录**：侧栏、状态流、未读、持久化
- [docs/plan.md](docs/plan.md) —— 开发计划：架构、技术选型、参考项目、里程碑、当前能力与缺口
- [docs/terminal-gap-analysis.md](docs/terminal-gap-analysis.md) —— **终端差距评估（2026-11-04）**：官方能力对照、代码证据与优先级建议
- [docs/optimization-plan.md](docs/optimization-plan.md) —— **当前优化计划**：主力版本/前沿版本、执行阶段、依赖与验收门槛
- [docs/gui.md](docs/gui.md) —— **前端路线**：主流终端怎么选 UI、四个候选对比、决定与验收
- [docs/remote.md](docs/remote.md) —— **SSH 方案**：远端后台进程、三种对接、安装与安全
- [docs/update.md](docs/update.md) —— **软件更新**：检查与安装、签名、发版步骤
- [docs/core.md](docs/core.md) —— **内核设计**：C ABI、快照/增量、语言选型（Rust）、测试策略
- [docs/protocol.md](docs/protocol.md) —— **协议契约**：客户端 ⇄ 宿主的消息表、错误码、能力与尺寸策略
- [docs/ui.md](docs/ui.md) —— **UI 规格**：侧栏分组/状态图标/相对时间/交互键位，附参考图
- [docs/tmux-integration.md](docs/tmux-integration.md) —— 历史资料：上一版（gmux）怎么跟 tmux 交互、哪些 tmux 配置会影响它
- [docs/kitty-fork-status.md](docs/kitty-fork-status.md) —— 历史资料：fork Kitty 阶段实测到什么、为什么放弃

两份历史资料描述的是上一个项目（gmux：fork Kitty + tmux）。pigeon 不再依赖 tmux，也不 fork Kitty，
它们留在这里只作为证据与反面教材。

## License

GPL-3.0-or-later（见 [LICENSE](LICENSE)）。派生作品（包括修改后的客户端与内核）必须同样以 GPLv3 开源。
依赖必须与 GPLv3 兼容（MIT / Apache-2.0 / BSD / LGPL 动态链接可用）。
