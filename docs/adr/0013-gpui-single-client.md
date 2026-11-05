# ADR-0013: 统一到单一 GPUI 客户端，取代按平台原生的 AppKit/GTK 客户端

- 状态：进行中（M5+，逐阶段落地）
- 日期：2026-11-05
- 依据：[gui.md](../gui.md)、[ui.md](../ui.md)、[ADR-0007](0007-platform-native-clients.md)、[ADR-0011](0011-linux-rendering.md)

## 背景

[ADR-0007](0007-platform-native-clients.md) 决定客户端按平台写原生（macOS AppKit + Metal、
Linux GTK4 + OpenGL），其核心理由是“侧栏是产品主体，必须是原生控件”。代价是两份展示层，
且 Linux 迟迟未与 macOS 功能对齐（`docs/plan.md` §11、§9 风险表“Linux 等于第二份 UI”）。

本 ADR 推翻该方向：改用 **单一 GPUI 客户端** 覆盖 macOS / Linux（及将来 Windows），
以“一套 UI 跑多端”换取不再维护两份展示层。GPUI 是 Zed 的 GPU 加速 UI 框架（immediate+retained 混合）。

这是 [gui.md §2](../gui.md#2-四个候选) 当初否掉的“方案 C：单套自绘 UI”一类。推翻的前提是显式接受其代价：
侧栏、右键菜单、滚动、无障碍全部自绘，没有系统级原生控件外观。

## 决定

- 新增 `clients/gpui/`（crate `pigeon-gpui`），只依赖 `pigeon_core::api`（`default-features = false`），
  通过 GPUI 绘制 `docs/ui.md` 的 Harp 界面；替代 `clients/macos`（AppKit）与 `clients/linux`（GTK4）。
- 依赖方向不变：`客户端 → pigeon_core::api → core`。VT 语义、快照/增量、布局、选择、OSC 7501、
  搜索、OSC 8、协议编解码仍只在 `core/`，GPUI 客户端一行都不碰（AGENTS.md §3 不变）。
- 视觉规格仍以 `docs/ui.md` §2–§4 的 Harp 尺寸与色值为准；GPUI 版本在 `theme.rs` 复刻同一套 token，
  保证外观与参考图一致。

## 约束

- `clients/gpui/` 不出现 `unsafe`（crate 级 `unsafe_code = "deny"`）；无 FFI，GPUI 的 API 是安全的。
- GPUI 以 git 依赖引用 Zed 仓库，**pin 到具体 commit SHA**（AGENTS.md §9：不许开区间）。
  当前 `rev = "654efae4"`，含 `gpui` / `gpui_platform` 拆分。
- 平台无关的连接层（`host.rs` / `host/transport.rs`）与 Linux 客户端共用同一份语义，逐字移植、保持同步。
- `cargo build` / `cargo test` 在根仍只跑 `core`：`pigeon-gpui` 是 workspace 成员但不进 `default-members`，
  用 `-p pigeon-gpui` 构建（与 Linux 客户端同规则，AGENTS.md §2）。

## 许可证

GPUI 为 **Apache-2.0**，本项目为 **GPL-3.0-or-later**（[ADR-0010](0010-gpl-3-0-or-later.md)）。
Apache-2.0 → GPLv3 是 FSF 认可的**单向兼容**：Apache-2.0 代码可链接进 GPLv3 作品并以 GPLv3 分发。
本项目只依赖这一方向（不反向），因此合规。Zed 其它 crate 的许可证随依赖树逐个核对。

## 工具链前提（macOS）

GPUI 的 macOS 渲染用 Metal，编译 shader 需要：
1. 完整 Xcode（非仅 Command Line Tools）：用 `DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer`，
   或 `sudo xcode-select --switch`；
2. **Metal Toolchain 组件**（较新 Xcode 按需下载）：`xcodebuild -downloadComponent MetalToolchain`。
与 `swift test` 需要完整 Xcode 是同类前提（AGENTS.md §2）。

## 取代与何时重开

- 取代 [ADR-0007](0007-platform-native-clients.md) 的“按平台原生”方向与 [ADR-0011](0011-linux-rendering.md)
  的 GTK4/OpenGL 渲染方向；两份旧客户端在迁移对齐完成前并存，完成后删除（去留以 `docs/plan.md` §11 为准）。
- 何时重开：若 GPUI 的 Linux/Wayland 或 IME 成熟度无法支撑 `docs/ui.md` 的输入法与无障碍要求，
  或其 pre-1.0 破坏性变更的维护成本超过收益，则重新评估是否回到按平台原生。
