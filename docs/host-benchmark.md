# 宿主协议性能基线

核对日期：2026-11-05。对应 [优化计划 S0](optimization-plan.md#3-s0基线与验收设施)。
工具为 `tools/bench/host-bench.py`，Rust 客户端只使用 `pigeon_core::api`，独立开发包不进入产品或默认 workspace 构建。

## 复跑

```sh
python3 tools/bench/host-bench.py --out /tmp/pigeon-bench-result
python3 tools/bench/host-bench.py --samples 100 --panes 16 --bytes-per-pane 1048576 --out /tmp/pigeon-bench-16
cargo fmt --manifest-path tools/bench/host/Cargo.toml --check
cargo clippy --manifest-path tools/bench/host/Cargo.toml --all-targets --locked -- -D warnings
```

需要 Rust、Python 3、POSIX PTY；输出目录必须不存在。脚本构建 release 宿主和独立 Rust 客户端，
使用短路径的 0700 临时目录、独立 HOME/配置、Unix socket 和 raw PTY Python fixture，
不启动用户 shell、不读取用户 rc、不连接已有宿主。正常退出和错误路径均尝试发送 Shutdown，结束自身宿主的 pane。
只有没有响应的宿主才最终 kill；输出日志留在指定目录。

`host/Cargo.lock` 固定独立工具依赖；更新核心依赖时同时核对两份 lock 中共同包的版本，避免测量不同版本的编解码器。

## 测量与校验边界

| 样本 | 起点 → 终点 |
|---|---|
| `handshake_us` | Unix socket connect 前 → 收到 hello_ack |
| `ready_us` | 发送 new_session 前 → 创建 pane、attach 并收到 fixture 的 READY |
| `echo_us` | 编码/发送 Keys 前 → 接收、解码并应用包含对应 ACK 的屏幕/增量 |
| `recovery_us` | 关闭客户端连接前 → 新连接握手、依次 attach 全部 pane 并应用快照 |
| `burst_completion_us` | 给所有 pane 依次发送输出指令前 → 各 pane 的最终 DONE 出现在应用后的屏幕 |
| `burst_us` | 同上 → 所有 pane 的 DONE 都出现 |

单位均为微秒。回显只探测第一个 pane，先预热 5 次，再记录指定样本数；fixture 的输入计数与请求序号必须一致，
末尾标记出现后不再写输出。重连比较全部格子（含组合码点/宽字符）、光标、尺寸、备用屏、滚动区及 seq；不相等便失败。
顺序 delta 被应用，已包含在快照中的旧 delta 被忽略并计数；向前缺序、非法行或 host error 直接失败，避免把数据丢失算作低延迟。

多 pane 输出是 **1–16 个独立会话，每个一个 80×24 pane**，均由同一个客户端订阅，非 GUI 分屏。
每个 fixture 输出指定数量的 ASCII 内容（含 CRLF），再追加 DONE 标记。各 pane 同时运行，但指令顺序发送；
结果包含发送偏差、fixture Python、PTY、VT、宿主调度、分帧、客户端解码和 API 应用的成本。
正常协议合并中间画面，因此 wire bytes、delta 数不等于程序输出字节数；工具验证最终标记和状态，不验证每个中间画面曾被显示。
长输出期间客户端每 15s ping；单次输出等待上限 120s，避免无界挂起。

此工具**不测 GUI 首帧、按键到显示器延迟、GPU 帧率、真实多窗口分屏、SSH RTT、CPU/功耗或 RSS**。
短时突发负载不替代 16 pane × 1 MB/s × 10 分钟、后台/遮挡、弱网或长时间稳定性测试。
CI 在 Ubuntu/macOS 运行 2 pane、10 次回显及每 pane 64 KiB 的 smoke，保存原始 artifact；无跨 runner 时间阈值。

## 产物与验证

- `environment.json`：提交、工作区状态、CPU/OS/工具链和白名单环境变量。
- `samples.json`：全部回显样本、每 pane 创建/完成时间、快照一致性结果、接收字节/帧数、release 二进制和 fixture 的 SHA-256。
- `host.log` / `client.log`：失败诊断；没有成功的 samples 文件不能当作通过。

2026-11-05 主线程在 Arch Linux x86_64 对单 pane 和 16 pane 各 1 MiB 短时输出完成实跑，
包括回显计数、Unicode 屏幕快照重连一致性；独立工具 fmt/clippy 通过。
具体数据需连同原始样本和环境读取；并行编译/Agent 会影响本机数值，不作为专用机器的性能承诺。
新增 CI smoke 的托管平台结果在 [执行记录](optimization-progress.md) 单独登记。
