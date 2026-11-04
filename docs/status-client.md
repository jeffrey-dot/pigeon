# macOS 完整状态集合客户端

核对日期：2026-11-05；父线程基线 `3c16b621daa14ea0777689f0f5b377cb2465362b`。
本片只修改 macOS 客户端、对应测试及检查脚本。宿主与接口见 [状态同步](status-sync.md)
和 [协议](protocol.md)，既有磁盘格式与 legacy 路径见 [持久化记录](status-persistence-progress.md)。
本页记录当前实现与验收范围；集中能力清单见 [plan.md](plan.md)。

## 当前实现

- hello 同时请求 `status_all` 与 `status_snapshot`，只有 hello_ack 包含后者才接受完整状态集合。
  未协商时继续使用旧 `status`；重连、断线清除能力标记，下一次握手重新协商。
- Swift 只调用 `pg_msg_status_snapshot`、`pg_status_snapshot_info/at/free`。
  所有字符串在 handle 释放前复制成拥有所有权的纯数据，不实现 CBOR、不重复内核生命周期。
  revision 和 age_ms 均保留 UInt64。
- 同 host、instance、pane 的首帧建立 revision 基准，此后只接收更高 revision。
  `wire` 更换连接 generation 并调用 `awaitReports` 重置基准；旧 transport 回调仍由 generation 与 client 对象过滤。
  同 revision 的新连接首帧合法；record seq 不用于比较集合新旧。
- 每帧原子替换该 pane 的完整记录集合，终态未读是当前集合减去本客户端已查看身份。
  缺失记录、空集合、同 id 新 seq 会撤下对应待处理项，绝不因此写 seen 或发送 ack。
  合法 idle 与没有记录分开；空 pane 不会遮住兄弟 pane 的明确报告。
- 各 pane 按 `blocked > error > done > working > idle` 派生当前报告，再聚合会话；同优先级按 pane、record id 稳定排序。
  会话保留来源 pane 与具体 record 身份。同 pane 多个 blocked 全部可选，不只缓存一个赢家。
- `sidebar.json` 沿用 `(host, instance, pane, seq, id)` 的 unread/seen 身份，不保存 revision 或可查看证明。
  已 seen 的终态重播可重发 ack；重复 revision 不重发。同一 instance 重启客户端仍去重；新 instance 清除旧身份。
  legacy 计数不迁移成 seen，已有偏好与旧宿主行为保留。

## 查看与确认

侧栏维持 Harp 单行布局。Session 菜单和侧栏右键菜单提供 **Status Records…**，打开所选会话的非激活面板。
协商完整集合时，**Next Needs You / ⌘J** 同时打开面板；原 **Mark as Read** 转为打开面板，要求逐条查看。

面板下拉列表逐条列出 pane、record id、状态；选中后以纯文本显示 app、kind、完整 msg，长文本可滚动。
选择会请求取消 zoom 并聚焦目标 pane；不自动退出回看。working/blocked/idle 只能查看，不提供有效确认。
对 done/error，用户点击 **Mark This Record as Read** 才产生该具体记录的 ack。

- 完整集合路径不因 pane 可见而自动确认。显示一个聚合 badge 不代表用户查看了该 pane 中所有记录。
- 确认继续要求当前连接握手完成、host/session 与实际布局匹配、应用与主窗口前台可见、pane 有当前连接
  `receivedScreen`、非空网格与 Metal renderer、实时屏、非隐藏/非 zoom 隐藏。面板自身也须可见。
- 面板绑定 host、instance、session、connection generation；断线、重连、切换会话使旧面板失效。
  同 id 被更新或记录被移除会清空选择，不自动选下一条。按钮点击时重新核验集合中的具体身份与可见性。
- ack effect 保留 host、instance、pane、seq、id，发送前选择并核验目标连接；先持久化 seen，再发送。
  本地确认仅移除对应待处理项，其他终态和 live 记录不受影响。
- 旧宿主仍按当前单条终态与原有可见性规则自动确认；面板只能显示该宿主实际报告的当前记录。
  legacy 未报告的历史身份仍保留未读，不从另一条消息推断删除。

## 本机验证

Linux，无 AppKit；全部测试编译真实源文件，未用替代实现或 stub。

```sh
export SWIFT=/tmp/pigeon-swift-toolchain/usr/bin/swift
export LD_LIBRARY_PATH=/tmp/pigeon-swift-toolchain/compat:/tmp/pigeon-swift-toolchain/compat/usr/lib/x86_64-linux-gnu
tools/checks/swift-logic.sh
cargo build --lib --offline
tools/checks/swift-status-wire.sh
cargo test --manifest-path core/Cargo.toml --offline
```

- Foundation：97 tests，0 failures（原有 86 条加 11 条）。先验证缺失集合/确认/面板接口失败，
  空集合来源与 legacy 面板确认还先验证了断言失败，再修复。
- ABI adapter：2 tests，0 failures。真实 Rust 库、生成头和 Swift adapter；涵盖原 msg 释放后读取 handle、
  handle 释放后 Swift 值可用、UTF-8、超过 UInt32 的 revision/age、错误消息类型和畸形缺字段。
- 全部 153 个 macOS Swift 文件 `swiftc -frontend -parse` 通过；仅语法检查，不代表 AppKit 类型检查。
- core 回归：278 passed、0 failed、2 ignored；本片未修改 core、协议或生成头。
- 文档相对链接、shell 语法与 `git diff --check` 纳入交付检查。

## 未验证与风险

`8ccaeb3` 的 [macOS 15 Intel CI](https://github.com/jeffrey-dot/pigeon/actions/runs/41888692090) 已通过完整 `swift build` 与 `swift test`（352 项、7 skipped、0 failures）。
Apple Silicon 最终版本、AppKit 面板布局/点击/焦点、真实 Metal 呈现和 SSH 断线仍需验收。
非激活面板设计用于保留主窗口焦点；真实 macOS 上仍须确认下拉与按钮交互、长文本滚动及可见性不会误判。
`receivedScreen` 证明当前连接数据已应用，仍不是 GPU 呈现完成回执。

seen 身份仍随长寿命 pane 增长；磁盘保存沿用主线程 best-effort 原子写，写盘失败、多个客户端进程同时写同一文件
不保证持久化一致性。宿主集合是共享的，另一客户端 ack 会使记录消失；此客户端只撤下活动项，不冒充自己已查看。
本片未扩展 Linux 客户端、OSC 生命周期、容量上限或宿主磁盘恢复。

## macOS 手动验收

1. macOS 执行 `cargo build && swift build && swift test`，使用独立 `PIGEON_CONFIG_DIR` 启动客户端。
2. 同 pane 上报两个不同 id 的 blocked 和一个 done。侧栏应保持一行、blocked 优先、done 未读为 1；
   从 Status Records 面板逐条读到三个消息。选择 done 并确认后，只它消失，两个 blocked 均仍可选。
3. 同 pane 再上报 error 与 done。确认一条只减一个未读；选中记录后由 pane clear 该记录，面板应清空选择且不发 ack。
   clear 全集合后无状态图标和未读；重新上报先前删除身份不得被当作已 seen（可用协议夹具重播）。
4. zoom 隐藏目标 pane、切换其它窗口、最小化、进入回看：确认不得发生。选择记录后取消 zoom，
   回到实时屏、恢复前台且当前连接屏幕到达后可确认。面板长消息应完整可滚动、无富文本/链接解释。
5. SSH 断线/恢复时旧面板关闭；相同 revision 首帧恢复集合，旧回调不影响新连接。
   当前连接只收到状态但尚未收到屏幕时不能确认；补取快照后恢复。宿主重启复用 pane/seq 不串 seen。
6. 重启客户端验证未读去重及已 seen 重播 ack；连接不支持 status_snapshot 的旧宿主验证单条状态、自动确认和菜单仍正常。

主线程整合检查：Foundation-only 98 tests、Swift ↔ C ABI 2 tests 通过；选择占位项会清除原确认目标，避免下拉框和按钮目标不一致。AppKit 完整构建已由上述 CI 验证，真实窗口交互仍需手动验收。
