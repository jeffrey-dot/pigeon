# 优化实现与验收

核对日期：2026-11-05。对应 [优化计划](optimization-plan.md)；当前缺口统一维护在 [plan.md §11](plan.md#11-当前能力与已知缺口)。
本页列当前已整合切片与可追溯验证，不表示 S0–S7 整体验收完成。

## 已整合能力

| 范围 | 实现 |
|---|---|
| Linux GL | unsafe 编译期限定在 gl.rs；拥有型纹理/pipeline 自动释放、检查上传范围、恢复上下文；隔离 Xvfb driver 资源测试 |
| 基线/CI | GTK 构建、clippy、逻辑测试；独立 HOME/配置的宿主测试；环境记录、FFI import 与生成头文件检查 |
| 宿主性能工具 | 隔离 release 宿主与 API 客户端，保留协议回显、快照重连、多 pane 短时输出原始样本及二进制 hash；无 GUI 显示延迟承诺 |
| OSC 8 | 核心、宿主、Rust/C 接口保留拥有型 URI；macOS hover/Cmd+click 已接，http/https 白名单；链接取消和点击期间变化有回归，Linux Ctrl+click/hover 已接 |
| 文本查询 | 核心/宿主 literal 搜索、ASCII 大小写选项、有界分页和文本范围提取；独立 generation 拒绝陈旧坐标；macOS 搜索条/取消/上下命中/原子定位高亮已接，Linux UI 与 regex 待接 |
| 宿主 TERM | pane 固定 xterm-256color/truecolor；linux/dumb/xterm-kitty/未设置四种外部环境回归 |
| 客户端接口 | `api::delta_of` 与 C ABI 共用校验，FFI 只经 api；Linux 消费连续 delta，缺序/非法行重新 attach |
| 文本 | 附加码点跨快照/delta/历史/接口保留，软换行复制；历史按预算先预检再转换，FFI 校验后转移所有权；Linux 有界整形缓存、字体随所有者释放 |
| Linux 输入 | GtkIMMulticontext、候选窗定位、中文 commit；异步粘贴经宿主括号包装；复制焦点 pane 当前屏幕 |
| Linux 回看 | scrollback 协商、滚轮与快捷键回看、位置条；输入/IME/粘贴/尺寸变化回实时屏并恢复 delta；15s ping 保活 |
| 侧栏状态 | pane 最新状态聚合；事件身份按 host/instance/pane/seq/record 持久化；重播去重，ack 核验来源；完整集合显式确认要求具体记录选中、当前连接屏幕与前台可见性；旧宿主保留单条自动确认 |
| 宿主完整状态 | `status_snapshot` 能力与独立 revision，原子替换未确认集合；ack/clear/退出立即广播，两种客户端接口提供 owned 读取；macOS 已接入逐条查看/显式确认，Linux 本地侧栏与逐条显式确认已接 |
| 连接恢复 | generation 隔离旧回调；重连清空 pending 快照，缓存报告/旧屏幕不能确认新记录；status seq 不充当流修订号，接受宿主合法低 seq 回退 |

专题实现与测试见 [文本资源说明](text-followup.md)、[文本预算与字体回收](text-budget-progress.md)、[Linux 输入记录](linux-input-progress.md)、[状态持久化说明](status-persistence-progress.md)、[Linux 回看记录](linux-scroll-progress.md)。

## GitHub 验证记录

| 提交 | 运行 | 结果 |
|---|---|---|
| `7dd459a` | [41833307041](https://github.com/jeffrey-dot/pigeon/actions/runs/41833307041) | docs、header、Swift、Rust macOS/Ubuntu、GTK 全部通过 |
| `c7c1ec0` | [41843748579](https://github.com/jeffrey-dot/pigeon/actions/runs/41843748579) | 全部通过，含组合字符与文本资源收尾 |
| `7a4a71e` | [41845716261](https://github.com/jeffrey-dot/pigeon/actions/runs/41845716261) | 全部通过，含 Linux 输入整合 |
| `8ccaeb3` | [41888692090](https://github.com/jeffrey-dot/pigeon/actions/runs/41888692090) | 全部通过：Rust Ubuntu/macOS 15 Intel、GTK、Swift、header、docs，含状态完整集合客户端与复审修复 |
| `662644e` | [41944011109](https://github.com/jeffrey-dot/pigeon/actions/runs/41944011109) | 全部通过，含 Linux GL 所有权、编译边界和独立 Xvfb driver 资源测试 |
| `73813d5` | [41946816404](https://github.com/jeffrey-dot/pigeon/actions/runs/41946816404) | 全部通过，含 Ubuntu/macOS 宿主基线 smoke 与原始样本 artifact |
| `03b1a41` | [41951970329](https://github.com/jeffrey-dot/pigeon/actions/runs/41951970329) | 全部通过，含 OSC 8 复审、文本查询、Linux 侧栏/订阅隔离与 Swift ABI 夹具修复 |
| `61acd11` | [41953937127](https://github.com/jeffrey-dot/pigeon/actions/runs/41953937127) | 全部通过，含会话环境覆盖、分屏继承、API/C ABI 与真实 PTY 校验 |

已通过的平台 CI 对应 `61acd11`，含 session env、OSC 8、文本查询、Linux 侧栏和宿主基线 smoke；上报工具/架构检查新 CI 待核对。`8ccaeb3` 的 macOS 15 Intel `swift build` 与 `swift test` 通过：执行 352 项、跳过 7 项、0 failures；独立 Foundation 测试 98 项通过。
Apple Silicon 的最终提交尚未完成验收；不能用 Intel 结果代替。CI 构建和测试也不等于真实状态面板/SSH/GPU 呈现验收。

## 主线程本地验证

环境：Arch Linux x86_64、Rust 1.99.1、GTK 4.24.1；Swift Foundation-only 使用官方 Swift 6.1.2 Linux 工具链。

- 文本整合：Rust 257 passed / 2 ignored，GTK 24 passed / 1 ignored，Swift 32 passed；VT golden 8/8，fmt/clippy/头文件与链接检查通过。
- Linux 输入整合：GTK 31 passed / 1 ignored；独立 Xvfb、D-Bus、fcitx5 拼音与临时真实宿主实测通过，覆盖中文单次提交、Ctrl/Alt、两种粘贴、复制、分屏、取消待完成粘贴和失焦恢复。
- 状态复审：新增 pending 快照与合法低 seq 回退测试先失败，再修复；Foundation-only 86 tests 通过。AppDelegate 语法检查不等于 AppKit 类型检查或 GPU 呈现验收。
- Linux 回看整合：GTK 40 passed / 1 ignored，fmt/clippy/build 通过；主线程复跑输入与回看隔离桌面脚本，10,000 行、80ms 延迟、快速翻页、回底部 delta、输入/IME/粘贴、resize/zoom/备用屏/分屏全部通过。47s 阅读/ping 和能力退化另有子任务验收记录。
- 文本预算/字体回收整合：主线程复验核心 262 passed / 2 ignored、无默认 feature 175 passed / 1 ignored、GTK 42 passed / 1 ignored；fmt、三组 clippy、VT 8/8 通过。合并后的独立回看桌面脚本全通过；256 次双 face 加载/整形/释放的 RSS 样本增长为 0 bytes，仅代表该测试环境。
- 宿主完整状态接口：整合核心 278 passed / 2 ignored；借用式 wire 校验调整后状态专题 7/7、C ABI 专题 3/3 和默认/无默认 feature clippy 通过。无默认 feature 整合回归 187 passed / 1 ignored，头文件 diff 与 GTK clippy 通过。两轴复审已结束：规范轴的缺口表同步与借用校验建议已落实，行为轴无新增可确认缺陷；macOS 完整集合已接入；主线程复验 Foundation-only 98 tests 和 Swift ↔ C ABI 2 tests，含占位选择清空确认目标的回归。完整 AppKit 构建与自动测试已由上述 Intel CI 通过，真实 GUI 尚需验证。
- macOS 完整集合客户端复审：规范轴仅发现 UI 文档未区分 legacy/完整集合，已修正；行为轴无可确认新增缺陷。Foundation-only 98 tests、Swift ↔ C ABI 2 tests 通过，真实面板交互未验。
- CI 默认使用明确的 `macos-15-intel`，手动 workflow_dispatch 可选择 `macos-latest` 运行 Apple Silicon 验证。切换原因是 latest 作业连续没有分配 runner 并被取消，不属于代码测试失败；两个架构的结果分别记录。
- 窗口按系统可点击内容边界安置侧栏按钮，原生标题栏命中测试通过；历史转换计数使用稳定线程局部接口，macOS Rust clippy/test 通过。
- Linux GL 边界：编译 deny 先报告 atlas/renderer 的 8 处越界 unsafe，统一封装后 fmt/clippy 通过；GTK 常规 42 passed / 2 ignored，独立 driver 测试通过，输入/延迟回看桌面脚本全通过。详见 [资源验证](linux-gl-resources.md)，对应 `662644e` CI 已全部通过。
- 宿主基线工具：release 单 pane 与 16 pane 各 1 MiB 短时输出、100 次回显和完整快照重连校验通过；纯工具 fmt/clippy 通过。测量边界、产物与复跑见 [host-benchmark.md](host-benchmark.md)。
- OSC 8 整合复审：规范轴发现 CAN/SUB 前缀取消没有关闭旧链接及校验重复；行为轴另发现重复 `flagsChanged` 声明和点击期间目标恢复后仍可打开。父线程修复并共用表校验；Rust 默认 293 passed / 2 ignored、无默认 195 passed / 1 ignored，Foundation 102 tests、两种配置 clippy、VT 8/8 通过。完整 AppKit 构建/测试已在 `03b1a41` CI 通过，真实 GUI 待验，见 [OSC 8](osc8.md)。
- 文本查询整合：两轴均复现行尾 TAB 未归一化导致额外空格和跨行漏匹配，先失败后修复；PTY 阻塞读写装配拆至 `pty_io.rs`，actor 保持串行状态所有权且降至 361 行。默认 core 314 passed / 2 ignored，无默认 211 passed / 1 ignored，GTK 42 passed / 2 ignored；查询专题 15/15。专题契约与边界见 [文本查询](text-query.md)。
- 验证脚本在 `tools/checks/`；本机测试不修改用户 shell/tmux 配置或操作用户已有终端。

## 尚未完成的验收与能力

1. 最终提交的 Apple Silicon 验证；真实 macOS 窗口激活、遮挡、回看、zoom、跨主机与 GPU 呈现/SSH 恢复验收。
2. Linux 跨重启 seen/偏好持久化，身份集合有界存储、多进程同时写 JSON 的一致性；两端完整集合与逐条确认已接。
3. 密集整屏文本的 8 MiB 上限处理、整帧编码/引擎内部附加码点预算、跨 cell ZWJ 字素宽度及宽字符折行占位。
4. Linux 拖选/SSH 管理与渲染脏行；真实 Wayland、IBus、日文 IME、非美式键盘和混合 DPI 验收。
5. 性能、弱网、功耗、长时间负载、完整 VT 属性/fuzz 覆盖及签名发布。

Linux 侧栏整合复审：两轴发现后台未发现 pane 状态误清、legacy 身份替换/容量、纯派生归属及会话/呈现时序缺口。父线程修复后 GTK 64 passed / 2 ignored；多会话、输入法和延迟回看独立桌面脚本通过。宿主控制消息顺序测试要求保持全部中间布局，内部订阅截止序号同时保留该保证。核心完整 315 passed / 2 ignored，GTK 64 passed / 2 ignored，GL 资源独立检查通过；`03b1a41` 整合 CI 全部通过。

`607759e`、`b1e4925` 的 Rust 双平台、GTK、header、docs 已通过；Swift 应用构建通过，但测试夹具缺少新 ABI 的 link_count/links 导致 swift test 编译失败，已补齐，并由 `03b1a41` 的 Swift CI 验证通过。

2026-11-05 session env：宿主真实 PTY 复现忽略覆盖与非法请求创建成功，再修复；验证 Unicode/空值/继承/分屏/会话隔离及拒绝 NUL/非法名称/保留变量。新增安全 API 与 C setter 原子构建，session_env 能力可探测。无 GUI 配置入口，未做运行中环境修改。

Session env 本机验证：默认 core 320 passed / 2 ignored，无默认 feature 214 passed / 1 ignored；API 2 项、C ABI 1 项、真实宿主 2 项专题通过。默认/无默认 clippy、无默认库构建、FFI import 与 cbindgen 通过；GNU release 1,993,536 bytes。之后追加能力探测与 128 项/32 KiB 精确上限断言，专题复跑通过；最终平台 CI 待验。

架构检查：core 直接依赖评审清单、host optional feature、客户端禁止打开内部 feature、核心 host-runtime/OS 分支和 Swift UI/CoreText import 接入 CI。4 组违规夹具与当前树检查通过；脚本是源码/manifest 检查，不替代编译器、传递依赖/许可证审计。

OSC 7501 上报工具：pigeon-report/pigeon-run 与接入指南已整合。规范轴发现选项样式字段和 dry-run 写失败码，行为轴另发现终态预算与 builtin 改写 trap；四组复现先失败后修复，21 组真实 CLI/API/PTY 测试通过，新增双平台 CI。共享能力表已同步，第三方 agent/远端/桌面通知仍未验。

搜索与 Linux 现代输入整合：核心 327 passed / 2 ignored，无默认 220 passed / 1 ignored、Swift 搜索模型/真实 ABI 7 项通过。搜索复审三项行为/类型问题已修复；Linux 复审五项事件转换问题已修复并补真实 PTY/GTK 回归。新增输入/链接契约见 linux-modern-input.md；整合 CI 待验。

主线程完整回看验收复现隐藏位置条引发同步 pointer-enter 重入 Workspace 的 panic；指针进入/离开在借用冲突时延后至当前 GTK 更新之后处理，保留事件而不在回调边界 unwind。已有 Return to live→再次翻页脚本作为回归。

本轮最终本地结果：GTK 77 passed / 2 ignored；modern-input、input、sidebar 桌面脚本通过，修复重入后 scroll 与 modern-input 再次通过。上报工具 06757e8 CI 的 Ubuntu、Swift、GTK/header/docs 通过，macOS Rust 作业的工具测试因夹具硬编码 /bin/true 失败；已改为 /bin/sh -c exit，等待新的平台运行。

2026-11-05 按用户 Harp 参考图补 Linux 界面：浅灰窄侧栏、单行名称/时间/状态槽、悬停关闭、选中白色圆角、终端卡片与顶部栏、按需记录浮层、浅深色。真实 GTK 截图已检查；GTK 79 passed / 2 ignored，clippy/fmt 与 GL driver 检查通过；input/scroll/modern-input/sidebar 桌面脚本通过。浅色暴露的 framebuffer alpha 与单格字形定位已修；标题栏 surface 偏移纳入鼠标坐标。见 [Linux Harp](linux-harp.md)。

6489ca6 的 Swift 完整构建/测试、Ubuntu Rust、GTK、header/docs 已通过；macOS Rust 的真实文本查询测试在 READY 后收到交互 shell 启动序列而正确返回 stale_text。测试夹具改为 READY 后安静读取的非交互进程，专题本机通过，待新 CI。
