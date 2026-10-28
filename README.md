# pigeon

一个原生终端：自带一个**隐形的后台会话进程**（关窗口任务继续跑，像 tmux，但不需要你配置任何服务），
以及一等公民侧栏（agent inbox）。
**M0 已可运行**：Rust 内核 + `pigeon-host` + AppKit 单 pane 客户端（关窗口进程不死、重开屏幕原样）。

- [AGENTS.md](AGENTS.md) —— 给编码 agent 的工作说明：**模块划分**、风格、测试、不变量
- [docs/m0-checklist.md](docs/m0-checklist.md) —— **M0 验收记录**：自动化结果、截图证据、未验项
- [docs/m1-checklist.md](docs/m1-checklist.md) —— **M1 验收记录**：分屏、拖动分隔线、关窗重开
- [docs/m3-checklist.md](docs/m3-checklist.md) —— **M3 验收记录**：侧栏、状态流、未读、持久化
- [docs/plan.md](docs/plan.md) —— 开发计划：架构、技术选型、参考项目、里程碑、验收与风险
- [docs/gui.md](docs/gui.md) —— **前端路线**：主流终端怎么选 UI、四个候选对比、决定与验收
- [docs/remote.md](docs/remote.md) —— **SSH 方案**：远端后台进程、三种对接、安装与安全
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
