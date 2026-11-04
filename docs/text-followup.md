# 文本切片质量与资源边界补充

2026-11-04，基线 `491aeb681e4298f7bcd119e02971e07a76d9850d`，独立 worktree 实施。
本页记录 [ADR-0012](adr/0012-cell-text.md) 的后续修复与验证；当前产品缺口仍以
[plan.md §11](plan.md#11-当前能力与已知缺口) 为准。

## 已实现

- `term/alacritty.rs` 对制表符格子的主码点和完整文本统一使用导出的空格。
  复现输入为 `HT CR CSI C U+0301`：引擎把组合符附到保留了 HT 的格子上；修复前
  `snapshot_of` 对核心自己的输出返回 `InvalidArg`，修复后快照和 delta 均为 `空格 + U+0301`。
- `session/cell_text.rs` 接受仅重复主码点的合法 text 项，但不分配 `extra_text`，保留普通格子的快路径。
  空字符串仍非法。坐标、重复项、首码点、控制字符、宽字符续格和长度校验仍由核心统一负责。
- FFI 的 `OwnedCellText` 在快照/delta 完成构建前持有文本表和字节；delta 行缓冲也保留 Rust 所有权。
  错误返回可自动回收；所有可失败转换完成之后才转移给 C。快照本体也在释放网格所有权之前分配。
  现有语义校验已能拒绝超宽/重复 delta 行，本次不把这些被挡住的路径描述成已证实的外部可触发泄漏。
- Linux 保持每个已加载字体的 swash cache key，复用 `ShapeContext` 和 cell 整形结果。
  结果缓存最多 1024 项、计费最多 1 MiB（UTF-8 key、glyph 数组、每项 64 字节估算开销）；任一超限清空后写入。
  单个超限结果临时返回，不长期保存在缓存外；字体身份/字号变化失效。只保存 glyph id/位置，不持有图集坐标。
  颜色与已有的合成粗体不改变整形位置。
- [核心回归](../core/tests/text_followup.rs) 覆盖恶意表、空/重复/极端坐标、续格、未替换行、
  4096 字节边界/UTF-8 截断、C ABI 错误返回、旧帧与旧 reader、覆盖/擦除、历史与 backlog 计费。
  [macOS 回归](../clients/macos/Tests/PigeonAppTests/PaneDeltaTests.swift) 增补文本复制所有权、
  只清除被替换行和旧快照清除附加文本；本机尚未运行 AppKit 测试。

## 协议、所有权与资源审查结论

[协议](protocol.md#3-消息) 仍是 1.0 的可选字段扩展；cell 的 4/5 整数编码、PgCell 步长和 C ABI 头文件不变。
旧 reader 忽略 text 后仍获得主码点；新 reader 收到缺省/空 text 表时不保留旧附加内容。
`decode` 验证 CBOR/schema，`snapshot_of`/`delta_of` 验证图片语义；C ABI 走相同图片转换。
不新增客户端协议解析。对跨库 ABI，旧客户端配新库仍只读原有字段；新客户端配旧库须遵守结构 size 规则，
本次没有为旧动态库新增加载协商。

已有历史 2 MiB 估算预算和 backlog 512 Ki 格等价计费包含字符串，新增测试证实会淘汰附加文本密集的旧行/旧帧。
单格限制不是整屏预算：64×40 格、每格 4095 字节的快照仍可超过 8 MiB，`encode_frame` 返回 `Overflow`。
本次没有静默丢弃文本、分片或重新协商协议；这类极端画面仍需后续设计才能可靠传输。
历史转换先按基础格宽限制行数，再按实际文本费用裁剪，因此临时转换内存也不是严格的 2 MiB 上限。

4096 字节只约束导出/接收边界；引擎内部零宽附加码点未有完整内存预算。
Linux 字体加载仍沿用现有进程生命周期字节存储，反复加载字体的内存回收未在本次解决。
缓存 1 MiB 是结果条目计费；HashMap 容量和 swash 内部工作缓冲有额外开销，不是进程 RSS 上限。
不声称完成 hostile-font 验证或整形总耗时预算。

`👩‍💻` 经快照仍保留为第 0 格 `👩 + ZWJ` 与第 2 格 `💻`，光标前进 4 格；新增回归锁定这一现状。
[ADR-0012](adr/0012-cell-text.md) 和现有缺口表的“跨 cell ZWJ 整簇宽度未支持”仍准确。

## 验证

环境：Arch Linux x86_64、Intel i5-12400F、Rust 1.99.1、swash 0.2.10；未把本机结果记为 Ubuntu/macOS 验收。

| 命令/检查 | 结果 |
|---|---|
| `cargo test --manifest-path core/Cargo.toml` | 257 passed，2 ignored（基线 246 + 新增 11） |
| `cargo test -p pigeon-linux` | 24 passed，1 ignored；新增 3 项，ignored 是手动 release 整形基准 |
| `cargo fmt --all -- --check` | 通过 |
| `cargo clippy --all-targets -- -D warnings` | 通过 |
| `cargo clippy -p pigeon-linux --all-targets -- -D warnings` | 通过 |
| `cargo build --lib --no-default-features` | 通过 |
| `tools/vt-parity/run.sh` | 8 条已提交 golden 全通过 |
| `tools/checks/ffi-imports.sh` | 通过，只引用 api |
| cbindgen 0.27.0 重新生成并与 `include/pigeon_core.h` 比较 | 无差异 |
| 临时 Swift 6.1.2 工具链执行 `tools/checks/swift-logic.sh` | Foundation-only 32 passed |
| 本页相对文件链接检查与 `git diff --check` | 通过 |

Swift 命令：

```sh
SWIFT=/tmp/pigeon-swift-toolchain/usr/bin/swift \
LD_LIBRARY_PATH=/tmp/pigeon-swift-toolchain/compat:/tmp/pigeon-swift-toolchain/compat/usr/lib/x86_64-linux-gnu \
tools/checks/swift-logic.sh
```

整形基准（排除字体加载；10,000 次 `e + U+0301`、13 px；仅 CPU，不含图集/GL/GUI）：

```sh
PIGEON_FONT='DejaVu Sans Mono' cargo test --release -p pigeon-linux \
  repeated_cell_shaping_benchmark -- --ignored --nocapture
```

固定字体样本：未缓存 19.958938 ms，缓存 0.269060 ms，实际整形 1 次。
同机默认字体发现的样本为 151.075070 ms / 0.288581 ms；字体差异显著，因此复跑使用固定字体命令。
不据此声称整帧性能或生产延迟已验收。初始失败测试还分别复现了制表符快照拒绝、冗余字符串分配、
同一 face 的 swash key 每次变化；后续测试全部通过。

## 验收范围

核心文本转换、FFI 和 Linux 整形缓存通过上述自动化检查。macOS 完整构建、新增 AppKit 测试、
CoreText/GTK 实际绘制与 Ubuntu 运行结果以对应提交的 CI 和平台验收为准，不能用本机纯逻辑结果替代。
整屏帧预算、引擎内部组合码点预算、跨格字素和字体生命周期回收仍按“协议、所有权与资源审查结论”中的限制处理。
