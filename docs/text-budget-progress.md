# 文本导出预算与字体资源回收

2026-11-05，基线 `cc51ce22aec38519ef038b6ac7f2d8791c36ccc2`，独立 worktree 实施。
本页描述当前文本预算与字体所有权实现，沿用 [ADR-0012](adr/0012-cell-text.md) 的文本与格宽语义。
集中缺口见 [plan.md §11](plan.md#11-当前能力与已知缺口)，验证记录注明所用环境与基线。

## 历史导出：先计费，再转换

- `Term::snapshot_with_history_budget` 按字节计费导出历史。
  Alacritty 从最新历史行向前借用读取，每行计费通过后才纳入转换范围；遇到首个放不下的整行立即停止。
  预检不分配 cell/string，也不先构造一条可能超预算的行。实际转换只覆盖最终选中的连续后缀。
- `ExportedText` 统一计算导出主码点、附加码点前缀与 UTF-8 长度；计费和分配使用同一前缀，
  保持原有 HT→空格转换及单格 4096 字节、码点边界截断规则。普通单码点没有附加字符串分配。
  cell 颜色/属性转换归入 `term/alacritty_cell.rs`，不更改 VT 解析器。
- `FrozenTerm` 同样先选择历史切片再克隆，屏幕快照不克隆整份历史。
  `session` 使用同一个 `HistoryBudget` 口径生成消息。被保留行的文本、属性、宽字符续格、软换行均保留；
  预算不足只减少 attach 携带的历史，不修改可见屏幕，也不淘汰引擎中的滚动缓冲。
- 本次没有改动 `status`、`proto`、`api`、`ffi` 或宿主连接；客户端接口及生成头文件不变，无需 cbindgen。

### 上限的精确定义

`HISTORY_BUDGET = 2 × 1024 × 1024`，每个历史行的估算费用为：

```text
cols × 20 + 16 + Σ(每个 extra_text 的 UTF-8 字节数 + 32)
```

这是历史导出的估算计费上限，**不是 2 MiB 实际堆内存、RSS 或整帧编码大小上限**。
结果按从旧到新排列，始终是最新的连续完整历史后缀；若最新一行本身超预算，返回空历史，
不截断行、不跳过该行后塞入更旧的便宜行。会计预检只借用已有引擎数据；被拒绝行没有导出分配。
按此口径，导出历史的 cell 数不超过 `floor(B / 20)`，附加 UTF-8 总长不超过 B；
实际 Rust cell、Arc、Vec 容量与元数据、单格转换临时字符串仍有额外成本，历史转换空间随 B 有界。
生成协议对象、编码帧、原始引擎存储、屏幕和调用方主动索取完整 `snapshot()` 都不属于这个历史预算。

### 先失败再修复的证据

[session 回归](../core/src/session/history_tests.rs) 使用真实 Alacritty：150 行、8 列、每格 4095 UTF-8 字节，
最终可发送 63 行。修复前实际转换 150 行，测试失败；修复后只转换 63 行。
另一个 512 列密集最新行超过 2 MiB 计费的用例验证转换次数为 0，屏幕内容不变。
计数器只在测试构建存在，不进入生产。

[历史预算回归](../core/tests/history_budget.rs) 比较实时与 FrozenTerm，覆盖零预算、刚好容纳、少一个字节、
最新行过大、HT+组合符、CJK/variation selector、软换行、备用屏和原滚动缓冲不变。
既有 [text_followup 回归](../core/tests/text_followup.rs) 继续覆盖密集整屏帧超限、文本边界和 backlog 计费。

## Linux 字体：随所有者回收

`Face` 以 `Box<[u8]>` 持有字体文件字节，按需生成 `FontRef<'_>` 借用，
不新增 unsafe、自引用结构或依赖。度量在移动主 face 前计算，渲染、栅格化、整形的借用不能超出字体生命周期。
同一 face 保留初次解析的 swash `CacheKey`；移动所有者不改变 key，重新加载得到新 key。

现有 `ShapedCell` 的字体 key/字号失效逻辑保持原样；字体变化时清空条目并重建 `ShapeContext`。
结果缓存仍最多 1024 项 / 1 MiB 计费（UTF-8 key、glyph 数组、每项 64 字节估算开销），
不是整个字体库、swash 缓冲或 RSS 上限。单个超限整形结果仍仅临时返回。
本片解决重新加载后已释放字体的持续留存，不添加系统字体文件大小、字体发现或整形耗时限制。

[字体回归](../clients/linux/src/render/font_tests.rs) 在独立子进程中使用真实 DejaVu Sans Mono，
先预热 16 次，再执行 256 次加载/整形/释放，交替 13/26 px，每次包含主 face 与 fallback face。
跨轮保留同一 `ShapedCell`，覆盖字体 key 变化导致的失效和旧 context 释放。
使用 `/proc/self/status` 测量 RSS，允许最多 32 份字体字节的分配器保留/噪声，不依赖计数器模拟释放。

- 初始失败用例仅加载/释放一个 face：每份 343,140 bytes，256 次 RSS 增长 **88,080,384 bytes**。
- 最终两个 face 加整形的完整用例：256 次 RSS 增长 **0 bytes**。
- 另验证同一 face 移动后 key 稳定、释放再加载 key 不同；已有字号/字体变化失效与有界缓存测试继续通过。

测试明确要求发行版提供的 DejaVu 字体，不使用用户配置的字体名，也不提交字体二进制或新增许可证负担。
Ubuntu GTK CI 已安装 `fonts-dejavu-core`；Arch 对应 `ttf-dejavu`。缺少 fixture 时测试明确失败，不静默跳过。
字体发现只在测量前运行一次；测量针对真实 face 数据所有权和整形生命周期，不含字体发现扫描成本。

## 验证

环境：Arch Linux x86_64、Linux 7.2.14、Intel i5-12400F、Rust 1.99.1、GTK 4.24.1、swash 0.2.10。
下列是本 worktree 的当前检查，不复用其它 worktree 的日志，也不当作 Ubuntu/macOS 结果。

| 命令 | 结果 |
|---|---|
| `cargo test --manifest-path core/Cargo.toml` | 262 passed，2 ignored，含真实 PTY 宿主测试；本片新增 5 项 |
| `cargo test --manifest-path core/Cargo.toml --no-default-features` | 175 passed，1 ignored，含两个预算单元回归与客户端接口用例 |
| `cargo clippy --manifest-path core/Cargo.toml --all-targets -- -D warnings` | 通过 |
| `cargo clippy --manifest-path core/Cargo.toml --all-targets --no-default-features -- -D warnings` | 通过 |
| `cargo build --lib --no-default-features` | 通过 |
| `cargo fmt --all -- --check` | 通过；宏声明下的新核心模块另用 `rustfmt --edition 2021` 格式化 |
| `tools/vt-parity/run.sh` | 8 条已提交 golden 全通过 |
| `cargo build --release --bin pigeon-server` | 通过；1,789,520 bytes，小于 8,388,608 bytes 上限 |
| `PIGEON_FONT='DejaVu Sans Mono' cargo test -p pigeon-linux --locked -- --nocapture` | 33 passed，1 ignored；本片新增 2 项，字体子进程结果不重复计数 |
| `cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings` | 通过 |
| `cargo build -p pigeon-linux --locked` | 通过 |
| 相对文档链接检查、`git diff --check` | 通过 |

release 样本是本机 GNU/Linux 动态链接 ELF，**不是 musl 静态发布产物的验证**。
未运行 macOS `cargo test` / `swift build && swift test`、Ubuntu CI 或 GTK 实际 DPI 切换 GUI 验收。

## 剩余限制

- **密集整屏仍可超 8 MiB**：64×40、每格 4095 字节的既有测试仍应收到 `Overflow`。
  没有新增分片、协议协商或静默删除可见文本，不能把历史分配修复写成整屏传输问题已解决。
- **引擎内部零宽码点增长仍未受完整预算约束**：4096 字节只限制导出/接收边界。
  Alacritty 内部 `push_zerowidth` 仍可增长；未改引擎、未重写解析器。滚动缓冲行数上限不能替代组合码点内存预算。
- 跨 cell ZWJ emoji 整簇宽度、hostile-font / 整形总耗时预算与长期 soak 仍未验收。
