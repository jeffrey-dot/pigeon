# Linux 本地传输边界

2026-11-05 核对。传输实现位于 `clients/linux/src/host/transport.rs`，只使用核心 API 编解码；socket 路径仍由宿主 `--socket-path` 提供。

- 已连接 socket 的客户端握手期限为 5 秒；没有收到可用握手结果时关闭连接，不永久等待。
- 握手完成后每 15 秒 ping。45 秒没有收到完整、合法帧时视为断线；零散字节不能无限延长期限。
- socket 写操作设 5 秒超时；读写失败、非法帧、队列超量都关闭收发两端。窗口侧栏显示具体原因，停止新状态确认。
- 出站最多 256 个待发帧、8 MiB 编码字节（含 writer 正在写的帧）；入站最多 256 个待处理帧、16 MiB wire 字节计费。
  超限直接断线，避免静默丢掉一部分输入或继续使用缺序画面；断线前部分输入可能已发送，不能自动重放以免重复执行。
- 入站解码中的一帧、解析缓冲、Message 对象开销及图像/字符串堆布局不包含在 wire 计费中；这些数字不是 RSS 上限。
  产品既有单帧 8 MiB 限制仍由核心解码器执行。
- `--socket-path`/拉起宿主的子进程执行尚未加独立超时；上面的握手期限从 socket 连接建立后开始。

本地 socket 已接 [自动恢复与增量续传](linux-reconnect.md)，按 instance 和连接代次重建屏幕与状态确认；SSH、多主机、已发送输入回执或 socket 对端身份的新检查尚未由本片实现。

## 验证

```sh
cargo test -p pigeon-linux --locked
cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings
python3 -B tools/checks/linux-search-check.py
python3 -B tools/checks/linux-sidebar-check.py
PIGEON_SCROLL_IDLE_CHECK=1 python3 -B tools/checks/linux-scroll-check.py
```

真实 Unix socket pair 覆盖：计费释放和超量拒绝、静默握手、完整帧顺序、入站静默关闭两端、畸形帧、入站 257 帧积压及超大出站错误。
本机 GTK 共 89 passed / 2 ignored；搜索与侧栏独立桌面脚本通过。47 秒静止回看/ping 与恢复输出另跑，不用逻辑测试冒充长时负载。
macOS 核心未修改；Linux 托管 CI 与真实 Wayland 分别核对，未声称整个 S5 已完成。
