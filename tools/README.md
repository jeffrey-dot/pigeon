# tools/

`report/` 是用户主动运行的上报工具，可单独安装到用户目录；运行依赖、接入示例与支持边界见
[Agent 接入](../docs/agent-integration.md)。其余为开发期脚本，每个脚本自己定位仓库根，从任何目录运行都可以。

| 脚本 | 做什么 |
|---|---|
| `report/pigeon-report` | Python 3 标准库 OSC 7501 CLI；默认控制 TTY，显式 stdout/dry-run，不自动改配置 |
| `report/pigeon-run` | 用户主动运行的前台命令 wrapper/trap 示例；保留退出码，取消不报 done |
| `checks/report-check.py` | 隔离 HOME/PTY，真实 CLI 输出经 core API 验证记录、长度、生命周期与 wrapper 信号 |
| `build/bundle-app.sh` | 打 Pigeon.app；`UNIVERSAL=1` 出双架构（发行用） |
| `bench/environment.py` | 输出 JSON 环境基线：版本、OS/CPU/工具链、GTK、指定环境变量；字体/刷新率等必须手工补充 |
| `bench/host-bench.py --out /tmp/pigeon-bench-result` | 隔离 release 宿主，记录协议回显、快照重连和多 pane 短时输出原始样本；[测量边界](../docs/host-benchmark.md) |
| `checks/swift-search.sh` | 编译 Foundation 搜索状态模型与真实 C ABI adapter，验证取消/过期/隔离和定位快照所有权；不替代 AppKit/GUI |
| `checks/swift-status-wire.sh` | 编译真实 Swift 状态快照 adapter，链接 Rust 库验证 C handle 所有权与 UInt64 字段 |
| `checks/swift-logic.sh` | 在临时 SwiftPM 包运行真实的 Foundation-only 侧栏/选择测试（可用 `SWIFT` 指定编译器）；不替代 AppKit 验收 |
| `checks/linux-scroll-check.py` | 独立桌面与真实宿主验收回看、延迟答复、输入回实时屏、备用屏和分屏；可选 47s 保活/能力退化 |
| `checks/linux-gl-check.sh` | 独立 Xvfb 软件 GL 检查资源删除、上下文恢复和非法上传拒绝；CI 单独运行 |
| `checks/linux-search-check.py` | 隔离 GTK/真实 PTY 搜索定位、取消、Unicode 与旧宿主退化验收 |
| `checks/linux-reconnect-check.py` | 隔离代理断链/宿主重启验收，验证会话与画面恢复、离线输入不重放及新实例隔离 |
| `checks/linux-harp-check.py` | 真实宿主/GTK 的 Harp 浅色深色截图，独立桌面与 fixture；见 [界面验收](../docs/linux-harp.md) |
| `checks/linux-input-check.py` | 独立 Xvfb/fcitx5/宿主验收 IME、复制当前屏幕、异步粘贴与分屏焦点；不操作用户桌面 |
| `checks/ffi-imports.sh` | 禁止 FFI 直接引用核心内部模块 |
| `checks/architecture.py` / `architecture_test.py` | 检查直接依赖与 Swift/Rust 导入边界，并用违规夹具验证检查器；不替代编译器或传递依赖审计 |
| `bench/render-bench.sh` | 渲染基准：大文件 `cat` + 5 秒持续输出，读 `PIGEON_FRAME_STATS` |
| `checks/m0-gui-check.sh` | M0：单 pane、关窗重开（对应 [m0-checklist](../docs/m0-checklist.md)） |
| `checks/m1-gui-check.sh` | M1：分屏、拖动分隔线、焦点（[m1-checklist](../docs/m1-checklist.md)） |
| `checks/m2-gui-check.sh` | M2：宿主崩溃恢复、多客户端尺寸、真键盘（[m2-checklist](../docs/m2-checklist.md)） |
| `checks/m3-gui-check.sh` | M3：侧栏、状态、未读（[m3-checklist](../docs/m3-checklist.md)） |
| `checks/harp-ui-check.py --out /tmp/pigeon-ui` | 临时宿主与真实 OSC 会话，截取 Harp 风格浅色 / 深色主界面 |
| `checks/metal-gui-check.sh` | Metal 渲染：两种外观各截一张 |
| `checks/quit-on-last-session-check.sh` | 最后一个会话退出后宿主结束、应用退出 |
| `checks/helpers/` | 上面脚本用的 Swift 小工具：按 pid 点窗口、取窗口号 |
| `vt-parity/run.sh` | VT 对账：同一段字节喂内核与参考终端，逐行比较（见 [README](vt-parity/README.md)） |

`checks/osc8-abi.sh` 在 `cargo build --lib` 后用 C 编译器验证生成头文件、旧 PgCell 步长和 OSC 8 URI 所有权；设计与验收见 [OSC 8](../docs/osc8.md)。
