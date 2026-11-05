# Linux 本地连接恢复

2026-11-05 核对。基于 [传输期限与预算](linux-transport.md)，当前恢复对象为已连接过的本地 Unix socket。

- 连接关闭、写失败、非法帧、超时或队列超限后禁用输入及状态确认，取消 IME、待粘贴、搜索和指针手势。
- 侧栏显示 Reconnecting 与断线原因。按 1、2、4、8、16 秒重试，随后间隔保持 16 秒；socket/hello 在后台线程执行，不阻塞 GTK。
- 重连只访问首次解析的 socket，不执行 `--detach` 或替换宿主，不在发现空会话时自动创建新任务。
- 每次成功握手替换连接代次，旧连接队列不再分发。完整状态集合 revision 基准重置；同 host/instance 的本次应用内 seen 身份保留，旧记录面板选择失效；新 instance 清空旧身份。
- 同 instance 恢复原所选会话；会话已经关闭时选择仍存在的会话。画面、回看位置和输入目标重新建立，必须收到新连接的完整快照与布局后才能输入。
- 新 instance 清空所有旧 pane/画面/seq，并显示“Host restarted · previous tasks were not restored”。空宿主保留空窗口，由用户 New Session 明确创建。
- 不重放离线按键、粘贴或旧出站队列。写到一半的输入不能判断对端是否执行过，自动重放会重复操作。
- 协议版本拒绝或显式 hello_error 停止重试并显示原因；恢复中关闭窗口会关闭当前连接，后台握手即使随后成功也立即关闭，宿主任务继续。

本片使用**完整快照恢复**，尚未接 Linux `attach.since` 增量续传；不保留跨连接回看或搜索视口。连接替换时不把缓存画面作为新的查看依据。
首次启动连接/宿主命令仍走现有同步路径；用户主动 Stop Host、手动重试按钮、SSH 多主机与磁盘会话恢复均未由本片实现。

## 验证

```sh
cargo test -p pigeon-linux --locked
cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings
cargo build -p pigeon-linux --locked
python3 -B tools/checks/linux-reconnect-check.py
```

独立 Xvfb/配置/socket/真实 PTY 测试关闭透明代理，验证宿主进程不退出；重连屏幕中的中文与组合码点相同，选中会话不变，没有重复创建。
离线按键和 hello/layout 后、延迟完整快照前的按键不发送。随后重启测试宿主，确认 pane 编号复用不恢复旧任务、空宿主不自动新建；用户新建后可输入。
窗口在恢复中关闭后客户端退出，宿主仍存活。逻辑回归覆盖同实例/新实例、输入门槛、已读身份与旧确认目标失效。

本机 GTK 92 passed / 2 ignored；重连（含延迟快照）、搜索、侧栏桌面脚本通过。真实 Wayland、休眠唤醒、硬件/长时间断连、SSH 和 Ubuntu CI 分别记录，不按本地代理断链结果宣称完成所有弱网验收。
