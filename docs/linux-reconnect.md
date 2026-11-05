# Linux 本地连接恢复

2026-11-05 核对。基于 [传输期限与预算](linux-transport.md)，当前恢复对象为已连接过的本地 Unix socket。

- 连接关闭、写失败、非法帧、超时或队列超限后禁用输入及状态确认，取消 IME、待粘贴、搜索和指针手势。
- 侧栏显示 Reconnecting 与断线原因。按 1、2、4、8、16 秒重试，随后间隔保持 16 秒；socket/hello 在后台线程执行，不阻塞 GTK。
- 重连只访问首次解析的 socket，不执行 `--detach` 或替换宿主，不在发现空会话时自动创建新任务。
- 每次成功握手替换连接代次，旧连接队列不再分发。完整状态集合 revision 基准重置；同 host/instance 的本次应用内 seen 身份保留，旧记录面板选择失效；新 instance 清空旧身份。
- 同 instance 恢复原所选会话；会话已经关闭时选择仍存在的会话。只有实时画面可复用其 seq 请求 `attach.since`；历史、搜索、缺少 delta 能力、新实例都取完整快照。
  delta 必须连续且网格有效，缺序立即取全量。输入等待当前布局及快照/续传完成；续传可以没有新帧，由 attach 后同连接请求的 pong 证明该请求已串行完成。
- 新 instance 清空所有旧 pane/画面/seq，并显示“Host restarted · previous tasks were not restored”。空宿主保留空窗口，由用户 New Session 明确创建。
- 不重放离线按键、粘贴或旧出站队列。写到一半的输入不能判断对端是否执行过，自动重放会重复操作。
- 协议版本拒绝或显式 hello_error 停止重试并显示原因；恢复中关闭窗口会关闭当前连接，后台握手即使随后成功也立即关闭，宿主任务继续。

续传没有新帧时，pong 只授权使用已追赶的画面继续输入，不作为新状态查看证据。用户打开具体状态记录时会补取完整快照；只有新连接实际应用的 snapshot/有效 delta 且已绘制，才允许确认。
不保留跨连接回看或搜索视口。宿主 backlog 丢失或 resize 屏障可能使 `since` 退化为完整快照，由核心决定。
首次启动连接/宿主命令仍走现有同步路径；用户主动 Stop Host、手动重试按钮、SSH 多主机与磁盘会话恢复均未由本片实现。

## 验证

```sh
cargo test -p pigeon-linux --locked
cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings
cargo build -p pigeon-linux --locked
python3 -B tools/checks/linux-reconnect-check.py
PIGEON_TEST_MISSED_DELTA=1 python3 -B tools/checks/linux-reconnect-check.py
PIGEON_TEST_FULL_RECOVERY=1 python3 -B tools/checks/linux-reconnect-check.py
```

独立 Xvfb/配置/socket/真实 PTY 测试关闭透明代理，验证宿主进程不退出；重连屏幕中的中文与组合码点相同，选中会话不变，没有重复创建。
离线按键和 hello/layout 后、延迟完整快照前的按键不发送。随后重启测试宿主，确认 pane 编号复用不恢复旧任务、空宿主不自动新建；用户新建后可输入。
窗口在恢复中关闭后客户端退出，宿主仍存活。逻辑回归覆盖同实例/新实例、输入门槛、已读身份与旧确认目标失效。

本机 GTK 92 passed / 2 ignored；重连（含延迟快照）、搜索、侧栏桌面脚本通过。真实 Wayland、休眠唤醒、硬件/长时间断连、SSH 和 Ubuntu CI 分别记录，不按本地代理断链结果宣称完成所有弱网验收。

增量续传补充：真实 recorder 验证 `attach.since` 存在、静默续传无 snapshot、断线期间输出由 delta 补回；历史断线仍取完整画面，延迟恢复前输入不发送。逻辑测试区分“追赶完成”与“收到新屏幕”，并覆盖布局前 delta 缺序取消旧 pong 的授权。最终测试数量见集中 [执行记录](optimization-progress.md)。
