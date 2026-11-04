# 优化实现与验收

核对日期：2026-11-04。对应 [优化计划](optimization-plan.md)；当前缺口统一维护在 [plan.md §11](plan.md#11-当前能力与已知缺口)。
本页列当前已整合切片与可追溯验证，不表示 S0–S7 整体验收完成。

## 已整合能力

| 范围 | 实现 |
|---|---|
| 基线/CI | GTK 构建、clippy、逻辑测试；独立 HOME/配置的宿主测试；环境记录、FFI import 与生成头文件检查 |
| 宿主 TERM | pane 固定 xterm-256color/truecolor；linux/dumb/xterm-kitty/未设置四种外部环境回归 |
| 客户端接口 | `api::delta_of` 与 C ABI 共用校验，FFI 只经 api；Linux 消费连续 delta，缺序/非法行重新 attach |
| 文本 | 附加码点跨快照/delta/历史/接口保留，软换行复制；FFI 在全部校验后转移所有权；Linux 有界整形缓存 |
| Linux 输入 | GtkIMMulticontext、候选窗定位、中文 commit；异步粘贴经宿主括号包装；复制焦点 pane 当前屏幕 |
| 侧栏状态 | pane 最新状态聚合；事件身份按 host/instance/pane/seq/record 持久化；重播去重，ack 核验来源；新报告自动确认要求当前连接的新屏幕与前台可见性 |
| 连接恢复 | generation 隔离旧回调；重连清空 pending 快照，缓存报告/旧屏幕不能确认新记录；status seq 不充当流修订号，接受宿主合法低 seq 回退 |

专题实现与测试见 [文本资源说明](text-followup.md)、[Linux 输入记录](linux-input-progress.md)、[状态持久化说明](status-persistence-progress.md)。

## GitHub 验证记录

| 提交 | 运行 | 结果 |
|---|---|---|
| `7dd459a` | [41833307041](https://github.com/jeffrey-dot/pigeon/actions/runs/41833307041) | docs、header、Swift、Rust macOS/Ubuntu、GTK 全部通过 |
| `c7c1ec0` | [41843748579](https://github.com/jeffrey-dot/pigeon/actions/runs/41843748579) | 全部通过，含组合字符与文本资源收尾 |
| `7a4a71e` | [41845716261](https://github.com/jeffrey-dot/pigeon/actions/runs/41845716261) | 全部通过，含 Linux 输入整合 |

当前状态持久化与复审修复的完整 macOS 构建/测试需核对最终提交 CI；不能以任一先前提交的绿色结果替代。

## 主线程本地验证

环境：Arch Linux x86_64、Rust 1.99.1、GTK 4.24.1；Swift Foundation-only 使用官方 Swift 6.1.2 Linux 工具链。

- 文本整合：Rust 257 passed / 2 ignored，GTK 24 passed / 1 ignored，Swift 32 passed；VT golden 8/8，fmt/clippy/头文件与链接检查通过。
- Linux 输入整合：GTK 31 passed / 1 ignored；独立 Xvfb、D-Bus、fcitx5 拼音与临时真实宿主实测通过，覆盖中文单次提交、Ctrl/Alt、两种粘贴、复制、分屏、取消待完成粘贴和失焦恢复。
- 状态复审：新增 pending 快照与合法低 seq 回退测试先失败，再修复；Foundation-only 86 tests 通过。AppDelegate 语法检查不等于 AppKit 类型检查或 GPU 呈现验收。
- 验证脚本在 `tools/checks/`；本机测试不修改用户 shell/tmux 配置或操作用户已有终端。

## 尚未完成的验收与能力

1. 状态修复的最终 CI；真实 macOS 窗口激活、遮挡、回看、zoom、跨主机与 GPU 呈现/SSH 恢复验收。
2. 宿主多记录枚举/同步、身份集合有界存储、多进程同时写 JSON 的一致性；当前客户端仅知道已收到的记录。
3. 密集整屏文本的 8 MiB 上限处理、历史转换临时内存、引擎内部附加码点预算、跨 cell ZWJ 字素宽度及宽字符折行占位。
4. Linux 回看/拖选/侧栏/SSH 管理与渲染脏行；真实 Wayland、IBus、日文 IME、非美式键盘和混合 DPI 验收。
5. 性能、弱网、功耗、长时间负载、完整 VT 属性/fuzz 覆盖及签名发布。
