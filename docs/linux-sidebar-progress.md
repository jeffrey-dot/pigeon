# Linux 本地多会话与状态侧栏

核对日期：2026-11-05。基线 `99f420c9ed82c9519bd4e64a4dcee2ae6b63cd67`。
本片仅修改 Linux 客户端、独立验收脚本及本专题/平台 README；不修改核心协议、macOS 或字体/GL 实现。
集中能力清单由父线程整合到 [plan.md](plan.md)；本片不表示 [优化计划](optimization-plan.md) S5 已全部完成。

## 当前行为

- GTK 原生侧栏列出本地全部 sessions，按 git 根目录（包括 `.git` 文件的 worktree）或 cwd 末级目录分组；
  同名不同路径分开。按 `blocked > error > done > working > idle` 排序；无 OSC 7501 报告不显示状态/相对时间。
  未读是当前未确认 done/error 记录数，needs-you 按会话计数；行提示包含尺寸来源与观察客户端数。
- 新建使用当前会话目录；关闭整会话有确认对话框，只为该会话的每个 pane 发 `close_pane`。
  首次空宿主自动创建；关闭最后一个会话后空窗口仍可新建。关闭客户端不发送 close/shutdown，宿主任务继续。
- `Ctrl+Shift+T` 新建、`Ctrl+Shift+[` / `]` 按 session id 循环、`Ctrl+Shift+B` 折叠侧栏，
  `Ctrl+Shift+J` 循环 needs-you 会话并打开 Status Records；具体记录选择后跳到来源 pane。
- 原分屏/zoom、IME、屏幕复制、异步粘贴、回看与 15 秒 ping 保留。切 pane 保留各自历史位置；
  **切会话回实时屏**，取消 preedit/待粘贴，在目标布局和本次选择的新屏幕到达前无输入目标。

## 会话与输入隔离

`SessionModel` 是不依赖 GTK/IO 的 reducer：sessions、选择、布局、按 pane 屏幕缓存、请求 effects 在一处。
握手只消费 hello_ack，后续 sessions、状态、屏幕按顺序交给模型，不在创建窗口前丢掉消息。
选中 pane 的 snapshot 即使先于 layout 到达也缓存；布局尚未建立时不绘制或接受输入。

宿主只跟一份 session.layout；因此每次切换（包括切回已有缓存）重新 attach 所选会话，再 attach 它的布局叶子。
当前连接每次选择带独立 generation，并在 attach 前发送有序 ping 屏障：旧选择屏幕/布局不会重新启用输入。
切换重置旧 scroll 请求计数并请求 live，避免被屏障丢掉的旧答复让回看永久 pending。
后台 delta 缺序不触发 attach，避免夺回宿主布局订阅；下次选中时用新 snapshot 恢复。

后台状态观察仅用 `status_all`。协议没有 detach，已访问 pane 仍保留订阅/尺寸票直到连接结束；只向当前布局 resize。
每 2 秒请求 sessions 修正后台关闭/布局变化（当前宿主不是每种后台树变化都广播 sessions）。
尺寸来源按宿主 size_state 显示；客户端没有新增尺寸策略设置。

## 状态、查看与确认

hello 请求 `status_all` 和 `status_snapshot`；只有确认完整集合能力才接受它，旧宿主保留 status 路径。
完整记录通过安全 `status_snapshot_of` 读取，不复制 OSC/VT/CBOR 实现。每 pane 用 revision 比较完整集合，
同连接旧/重复 revision 忽略，新连接清除基准，host/instance 由连接上下文绑定；record seq 仅作为 ack 身份。
删除、空集合、同 id 更新撤下旧记录，不生成 ack 或已读证明。seen 仅保存仍在当前集合的显式确认身份，
没有跨重启磁盘持久化。旧宿主不能提供完整删除语义；最多保留每 pane 256 条收到的终态/当前报告，不自动确认。

Status Records 内选择具体记录显示完整 app/kind/msg 纯文本，长文本可滚动。选择只聚焦来源 pane、必要时取消 zoom；
不会自动退出回看。按钮仅允许 still-current 的 done/error，要求当前选择、连接、布局、已绘制屏幕、非回看、
非隐藏 pane、窗口前台且未最小化。点击时重新核验；同 id 新 seq、clear、换会话（即使 A→B→A）、断线都使旧目标失效。
已退出 pane 仍能复制/回看/关闭和确认终态，但不能输入。

## 验证方式与范围

```sh
cargo fmt -p pigeon-linux --check
cargo clippy -p pigeon-linux --all-targets --offline -- -D warnings
cargo build -p pigeon-linux -p pigeon-core --offline
cargo test -p pigeon-linux --offline
python3 tools/checks/linux-sidebar-check.py
python3 tools/checks/linux-input-check.py
PIGEON_SCROLL_IDLE_CHECK=1 python3 tools/checks/linux-scroll-check.py
PIGEON_TEST_NO_SCROLLBACK=1 python3 tools/checks/linux-scroll-check.py
```

Arch Linux、GTK4、Xvfb、Mesa 软件 GL、fcitx5、xdotool、PyGObject。所有 GUI 检查使用独立 HOME、XDG 配置/运行目录、
X display、D-Bus 和真实宿主；侧栏检查另建独立 accessibility bus。协议录制/夹具只调用 core 安全 API。
`PIGEON_SIDEBAR_SCREENSHOT=/tmp/sidebar.png` 可保存侧栏截图。

已先验证缺失接口的编译失败、旧 scroll pending 导致的行为失败，再实现。
最终 Linux 逻辑测试 **59 passed、0 failed、1 ignored**（既有字体性能基准）。fmt、clippy（`-D warnings`）、build 均通过。
真实侧栏脚本、输入脚本（新增 session 切换取消 preedit/待粘贴）、回看脚本与缺少 scrollback 能力分支通过；
回看含 47 秒静止阅读、至少三次 ping/pong、返回 live 后继续输出。无 GTK critical 或 Rust panic。
脚本环境的 Xvfb 无 DRI3、桌面 portal/FUSE 提示不影响软件 GL 与这些验收。
覆盖：屏幕先于布局、后台迟到布局/屏幕、重复切换、关闭 pane 迟到帧、完整 revision 与 seq 分离、实例/连接切换、
同 pane 多 blocked/终态、clear/空集合、精准单条 ack、旧能力、已退出 pane、后台状态不 attach/resize、
项目分组、可见性门控、session/pane 切换取消 IME/粘贴、真实进程保留和新客户端附加。

## 未实现或未验证

- 不实现 SSH、自动重连、pong 超时恢复、跨重启 seen/会话偏好持久化、宿主磁盘恢复或机器重启恢复进程。
- Linux 本片使用原生 GTK 文本状态和紧凑单行按钮；未对齐 macOS Harp 图标动画/材质、拖动侧栏宽度、组内重命名/新建菜单。
  相对时间取状态快照采样值；无新的宿主状态时不独立计时。窄行截断，tooltip 和逐条记录保留全文。
- GTK draw 调用完成作为本连接屏幕可见证据，未取得显示器扫描输出回执；未覆盖 compositor 遮挡等真实桌面全部行为。
  Wayland、IBus/日文 IME、非美式键盘、真实触控板、混合 DPI、硬件 GL 和打包发布未验。
- 未在本片执行 macOS Swift/AppKit、Ubuntu 托管 CI、SSH GUI 或核心全量回归；核心/VT 未改动。
- 非上报程序仍为未监控；不推测前台 agent 生命周期，不解析屏幕/标题推断任务。
