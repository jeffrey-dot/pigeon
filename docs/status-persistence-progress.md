# macOS 侧栏状态持久化执行记录

核对日期：2026-11-04。基线：`491aeb681e4298f7bcd119e02971e07a76d9850d`。
本项落实 [优化计划 S1](optimization-plan.md) 的客户端状态身份、确认与重播处理。
改动限定 macOS `Sidebar/`、`Net/Host/` 与 AppDelegate 接线；未修改核心、协议或 Harp 单行视图。
按并行 worktree 分工，本记录独立维护；[当前缺口](plan.md) 与共享执行记录由父线程整合。

## 已实现

- `sidebar.json.statusHosts` 按 host 保存 instance，再按 session、pane 保存未读和已查看身份集合。
  身份为 `(host, instance, pane, status seq, record id)`；没有 record id 的旧宿主用空字符串。
  status seq 与屏幕 delta seq 完全分开。重新加载、重复报告不会增加同一身份的未读。
- 已查看身份也持久化。宿主重播同一条终态时可重发 ack，不要求用户重新查看；这是对过去查看的重试，
  不会将隐藏 pane 的新终态当作已查看。仅 UI 刷新不重复发送 ack。
- ack effect 保留 host、instance、pane、seq、record id，发送时选择并核验目标连接，避免依赖临时 `activeHost`。
  新 instance 清除原实例的运行状态和持久化身份；旧连接排队中的消息及关闭回调由连接 generation 隔离。
  无 instance 的宿主每次握手使用不同的本地身份，不能跨重连安全去重，优先避免误确认。
- 仅持久化事件身份，不把磁盘缓存当作新连接已经呈现的状态。重连保留屏幕和未读，但缓存报告不能触发 ack；
  等新连接实际收到对应状态后再判断。会话列表删除 pane/session 时同步清理对应身份。
- 当前 pane 状态与未读集合独立。较低 seq 不覆盖仍然有效的较新报告，但其未读 done/error 仍会被保存；
  当前报告已确认、明确 idle/清除或重新连接后，可接受宿主露出的较低 seq 记录。
  已查看记录重播只重发 ack，不遮住当前 blocked/working 等记录。
- 自动确认只确认可见 pane 的**当前**终态，不批量确认该 pane 所有历史未读；新 working/blocked、idle、较高 seq
  都不会静默消除旧的 done/error。聚合仍是各 pane 当前最新状态的 `blocked > error > done > working > idle`。
- 确认要求连接握手完成、host/session 与实际布局匹配、应用和窗口获得关注、窗口可见、pane 已有快照、
  非 zoom 隐藏、具备非空视图和 Metal renderer、位于实时屏而非回看。不扫描屏幕内容推断状态。
  状态批量接收后在 run loop 检查当前报告，避免同批被覆盖的终态自动获 ack；菜单 Mark as Read 同样遵守可见性条件。
- `SidebarStore` 用 `Data.write(.atomic)` 支持首次创建和覆盖保存。持久化调用先于 ack 入发送队列。
  原 Protocol 文件超过 400 行，GUI 自测方法原样移入 `AppDelegate+SelfTest.swift`；协议处理保持独立。

## 旧格式迁移

旧 `unread` 只有会话计数，没有 host instance、pane 和记录身份，无法判断计数属于当前宿主还是已退出的进程。
保留该字段以便恢复检查，同时保留折叠、排序、会话改名、项目改名；packed order 继续按原规则迁移。
旧计数不映射到当前 pane，也不作为“已查看”或 ack 依据。宿主实际重播的终态建立新的可归属身份。
因此旧格式计数不能无损恢复：未被宿主再次发送的记录无法从一个总数重建。
JSON 格式缺损时保留可解码的偏好，不能解码的状态身份按空集合处理，不推断任何已查看记录。

## 验证

环境：Linux x86_64（Arch 内核 `7.2.14-arch1-1`）、官方 Swift 6.1.2。

```sh
SWIFT=/tmp/pigeon-swift-toolchain/usr/bin/swift \
LD_LIBRARY_PATH=/tmp/pigeon-swift-toolchain/compat:/tmp/pigeon-swift-toolchain/compat/usr/lib/x86_64-linux-gnu \
tools/checks/swift-logic.sh

cargo test --manifest-path core/Cargo.toml --offline
git diff --check
```

- `swift-logic.sh`：83 tests、0 failures，编译真实源文件与 XCTest，未使用 stub。覆盖 SessionReducer、身份存储、HostConnection、
  StatusVisibility、现有侧栏/选择逻辑；应用重启加载再重播用例先验证失败（未读 2，预期 1），修复后通过。
  较旧 working 覆盖较新 blocked、已读高 seq 重播遮住低 seq blocked 也先复现失败再修复。
  新增接口的重连/实例验证先验证缺失接口的编译失败，再实现并通过。
- 真实临时目录验证首次保存、覆盖保存、重新创建 store 加载；覆盖确认重发、低 seq 终态保留、同 pane 多记录、
  host/instance/pane/record 隔离、旧格式迁移、异常字段、pane 删除清理、后台和隐藏视图。
- `swiftc -frontend -parse` 对全部 AppDelegate extension 与 `PigeonApp.swift` 语法检查通过；这不等于 AppKit 类型检查。
- core 回归：246 passed、0 failed、2 ignored（含文档测试）。核心未修改。
- 相对 Markdown 链接检查采用 CI 同一规则；`git diff --check` 通过。

## 剩余限制与父线程验收

- 当前宿主只发送最新未确认记录，且 ack 本身不立即推送下一条记录。客户端只能保存**收到过**的终态身份；
  未收到的历史记录、同 pane 多条 blocked 的枚举和展开仍需要核心多记录协议，未在本项实现。
  更高 seq 不代表低 seq 已确认；保存的旧未读可以在 pane/session 删除或 instance 变化前持续存在。
- status 缺少独立修订号/清除代次。不能完全区分乱序旧帧与合法低 seq 回退；此实现保守保留未读，
  仅在上述明确条件下改变当前报告。宿主清除新 live 记录直接露出旧 live 记录的情况仍可能维持较新显示，
  需要重连重新取当前值或协议修订号才能消除歧义。
- 未读是收到过的未查看终态数量，当前状态仍是 pane 最新值；若旧终态被 working/idle 遮住，未读保留，
  但现有 needs-you/跳转规则未扩展为历史记录收件箱。Harp 单行 UI 保持原样。
- 已查看身份在当前实例内保留，pane/session 删除或 instance 切换时清理；长寿命 pane 持续上报会使身份集合与 JSON 增长。
  未设置会丢失去重证据的任意淘汰上限；压缩和有界存储需后续设计。写盘仍在主线程且按状态变化原子保存。
- 写盘失败仍按现有 best-effort 策略保留内存状态，不阻塞 UI；无法保证磁盘满、损坏或多客户端进程同时写同一文件时的跨进程一致性。
  不同客户端各自查看/确认的全局同步也没有新增协议保证。
- 本机无 AppKit。完整 `swift build && swift test`、窗口激活/遮挡、zoom、回看、同批状态与 GPU 真正呈现顺序、
  SSH 重连和宿主重启的真实交互需父线程 macOS CI/GUI 验收。run loop 延后不是 GPU 呈现完成回执。
- 已核对 README、AGENTS 当前状态、plan §11 和优化计划；共享状态页未在此 worktree 修改。
  父线程整合时应标明“客户端跨重启身份持久化已实现、纯逻辑已验，完整多记录协议和 AppKit 验收仍待补”。
