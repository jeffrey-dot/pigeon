# pigeon 词汇表（GLOSSARY）

> 一个概念只有一个名字。左列是正确的说法，「别用」列是撞过车的说法。
> 新概念先加进这张表，再写代码。

## 1. 产品与用户可见的概念

| 术语 | 定义 | 别用 |
|---|---|---|
| **项目** project | 侧栏最外层分组。取 git 根目录名；不是 git 仓库就取 cwd 末级目录名。由 `Sidebar/Grouping.swift` 算 | workspace、folder、repo |
| **会话** session | 一组 pane 的容器，拥有自己的布局树与 cwd。协议里是 `session_id` | tab、workspace、任务 |
| **窗口** window | 客户端的一个 `NSWindow`。窗口观察会话，不拥有它；多个窗口可以看同一个会话 | — |
| **pane** | 终端的最小单位：一个 PTY、一个进程组、一份屏幕状态。`pane_id` 从 1 开始，0 表示没有 pane | terminal、tab、view |
| **分隔线** divider | 两个 pane 之间的可拖动边界，由布局树的 `split` 节点 id 标识（`split_id`） | border、gutter |
| **放大** zoom | 一个 pane 临时占满会话区域，兄弟 pane 隐藏但继续运行。`unzoom` 还原原有嵌套比例 | maximize、fullscreen |
| **附加** attach | 客户端向宿主索取某个 pane 的当前状态：先快照，之后增量 | connect、open、join |
| **needs you** | 侧栏组头对 blocked、error、未读 done 会话的计数文案，英文原样 | 待处理、pending |
| **未读** unread | 会话有用户没看过的 `done`/`error`。与 `needs you` 是两个独立计数 | unread messages |
| **相对时间** age | 行尾 `1m` / 头部 `Working · 1m` 里的时长，来自 `status.age_ms`（时长，不是时间戳）。没有状态记录时留空 | timestamp、last active |
| **跳转** jump | ⌘J：循环选中下一个 needs-you 会话并聚焦其 pane | next、cycle |
| **输入法** IME | `NSTextInputClient` 的 preedit / marked text。preedit 只在客户端画，提交后才发字节给宿主 | composition |
| **退化模式** degraded | 远端三档对接的 C 档：只有普通 ssh，状态能显示但不持久 | fallback、plain mode |

## 2. 内核（`core/`）

| 术语 | 定义 | 别用 |
|---|---|---|
| **内核** core | `core/` 这个 crate：VT 状态机、布局树、状态记录、会话模型、协议编解码 | engine、backend |
| **`Term`** | 包住 VT 引擎的 trait（`feed` / `resize` / `snapshot` / `scroll`） | Terminal、VT 层 |
| **屏幕** screen | 当前可见的 `cols × rows` 网格，协议里是 `snapshot.screen` | grid、buffer、viewport |
| **备用屏** alt screen | DECSET 1049/1047/47 的全屏模式，`vim`/`less` 用它。备用屏下禁用回看 | alternate buffer |
| **滚动缓冲** scrollback | 屏幕之外的历史行，宿主固定 10000 行；嵌入配置上限 100000，超出按行淘汰最旧的 | history buffer |
| **回看** scroll | 把视口移到滚动缓冲某处：请求 `scroll{pane_id, offset}`，回答一屏 `snapshot` | paging |
| **历史** history | `snapshot.history` 这个字段，可在 `attach` 时下发；macOS 当前请求 `history:false`。与回看是两件事 | scrollback |
| **offset** | 请求中负数 = 往历史看、正数 = 往下、0 = 回实时屏；答复中为 `[0, history_len]` 的绝对回看行数 | position、line |
| **快照** snapshot | 某一刻的完整屏幕 + 光标 + 滚动区（+ 可选历史）。不可变，可跨线程持有 | frame、render、screen dump |
| **增量** delta | 快照之后只带脏行的帧，`seq` 单调递增 | diff、patch、update |
| **脏行** dirty row | 自上次发出后内容变过的行号，`pg_pane_dirty_rows` 的值 | changed row |
| **`seq`** | 每个 pane 独立的增量序号，从 1 开始。只有 `delta` 递增；`snapshot` 携带它已包含的最新 `seq`，自己不占号 | version、revision |
| **时钟基准** seq 基准 | 某客户端在某 pane 上已收到的最大 `seq`。`snapshot` 重置它；`hello_ack.instance` 变了就丢弃全部 | — |
| **格子** cell | 屏幕上一个字符位：`codepoint` + `attrs` + `fg` + `bg` + 可选 `link` + 推导出的 `width` | char、glyph |
| **宽字符** wide char | 占两格的码位（CJK、emoji）。第二格是续格，客户端按宽度跳过绘制 | fullwidth |
| **`attrs`** | 单元格样式位（粗体/斜体/下划线/反色/删除线/暗色/软换行）。软换行只在行末格标记；只加位、不改语义 | style、flags |
| **`modes`** | 终端模式位（`?1`/`?2004`/`?1000`…），告诉客户端方向键、鼠标、滚轮该发什么 | attr、flags |
| **滚动区** scroll region | DECSTBM 设的行区间。当前 VT 引擎不暴露它，恒等于整屏 | margin、viewport |

## 3. 状态（OSC 7501）

| 术语 | 定义 | 别用 |
|---|---|---|
| **状态 / `state`** | 五个值之一：`idle` / `working` / `blocked` / `done` / `error` | status、activity |
| **`status`** | 协议消息名（`t: "status"`）与 OSC 7501 的通道名。只指消息或通道，不指状态值 | — |
| **记录** record | 一条 OSC 7501 上报（`core/src/status/` 的 `Record`），有 `id`、`state`、`kind`、`msg`、`app` | entry、event |
| **`kind`** | `blocked` 的子类型：`permission` / `question` / `auth` | type、category |
| **`app`** | 上报这条记录的 agent 名（OSC 7501 的 `app=`），只当纯文本 | agent、program |
| **状态修订号** status revision | 每 pane 状态集合变更序号，`status_snapshot.revision`；ack/clear 也递增，与记录身份和屏幕 seq 独立 | status seq、时间戳 |
| **记录序号** record seq | OSC 7501 每次有效记录更新的身份；ack 使用它，不能用它比较集合报告新旧 | revision |
| **状态快照** status snapshot | 一个 pane 的完整未确认记录集合；整份替换，缺失即删除，空集合即清空 | 状态增量 |
| **确认** ack | 用户已看过该记录，可以被淘汰（`ack_status`）。对 `working`/`blocked` 无效 | read、clear |
| **清除** clear | OSC 7501 的 `clear` 指令：删除一棵子树。与 ack 是两条路 | ack、reset |
| **未监控** unmonitored | 没有任何 OSC 7501 记录时的显示：侧栏该 pane 不给状态 | unknown、idle |
| **聚合** aggregate | 按 `blocked > error > done > working > idle` 合并各 pane 最新状态并保留来源 pane，再派生项目计数；宿主支持同 pane 完整集合，客户端聚合接入待补 | rollup、summarize |

## 4. 宿主与协议

| 术语 | 定义 | 别用 |
|---|---|---|
| **宿主** host | `pigeon-server` 进程，持有 PTY 与会话状态。协议里的 `host_id` 指它 | server、daemon、backend |
| **客户端** client | 窗口进程（`Pigeon.app`） | app、frontend、UI |
| **连接** connection | 客户端到宿主的一条 socket 或 stdio 通道。一个客户端可以有多条；`client_id` 是每条连接一个 | session、socket |
| **`instance`** | 宿主进程的一次启动标识（`hello_ack.instance`）。变了意味着 `seq` 与 pane id 从头开始 | pid、epoch |
| **帧** frame | `u32` 小端长度 + 恰好那么多字节的 payload，上限 8 MiB | packet |
| **payload** | 一帧里的单个 CBOR map，其中 `t` 是消息名 | body、content |
| **能力** feature | 握手协商的功能名（`delta` / `scrollback` / `zstd` / `osc7501` / `status_all` / `status_snapshot`） | capability flag |
| **尺寸策略** size policy | 契约为 pane 级：`latest`（默认）/ `smallest` / `locked`；当前实现按会话投票，首次 attach 决定，差异见协议 §0 | resize mode |
| **尺寸来源** source | `size_state.source`：当前谁在决定尺寸，必须显示在侧栏 | owner |
| **背压** backpressure | 慢连接中同 pane 的待发 delta 合并为最新快照；出站字节超过 64 MiB 断开，客户端重连 attach | throttling |
| **续传** resume | 重连带 `attach.since = seq`：宿主补发还在缓冲里的增量，不发快照 | reconnect |
| **代理** proxy | `pigeon-server --proxy`：stdio ↔ socket 的帧桥 | tunnel、bridge |
| **a/b/c 三档对接** | 远端三个探测档位：A `--proxy`（推荐）→ B socket 直转 → C 普通 ssh（退化模式） | modes |

## 5. 架构词汇

来自 codebase-design，照抄不翻译。

| 术语 | 定义 | 别用 |
|---|---|---|
| **module** | 任何有 interface 和 implementation 的东西，不分尺度：函数、文件、crate、跨层切片都算 | unit、component、service |
| **interface** | 调用方要正确使用它必须知道的一切：签名 + 不变量 + 顺序约束 + 错误模式 + 配置 + 性能特征 | API、signature |
| **implementation** | module 内部的代码。焦点是缝时改用 **adapter** | — |
| **seam** | 不改动某处就能改变行为的位置，即 interface 所在的地方 | boundary |
| **adapter** | 坐在 seam 上、满足某个 interface 的具体实现。说的是角色，不是内容 | impl、driver、plugin |
| **depth** | interface 上的杠杆：调用方学一单位 interface 能撬动多少行为。不是实现行数除以接口行数 | complexity |
| **leverage** | depth 给调用方的好处：一份实现，N 个调用点、M 个测试都赚到 | reuse |
| **locality** | depth 给维护者的好处：改动、bug、知识、验证集中在一处 | cohesion、encapsulation |

两条判据：删掉这个 module，复杂度消失说明它是 pass-through，复杂度在 N 个调用点重新长出来说明它在挣钱；
一个 adapter 是假 seam，两个 adapter 才是真 seam。

本仓库的真 seam：`api` 与 `ffi` 是同一份语义的两个 adapter；`Term` 后面跟着当前引擎与将来的替身。

## 6. 禁用的说法

| 别写 | 写成 | 为什么 |
|---|---|---|
| tab | session | 没有 tab 概念：一个会话就是一屏 pane 树，侧栏的行是会话 |
| terminal | pane / 内核 / 客户端 | 同时指三样东西 |
| session | 连接 / 会话 | `session` 是 pane 的容器；到宿主的 socket 通道叫连接 |
| window | 会话 / `NSWindow` | 协议没有 window 层，写文档时要说明是哪一个 |
| API | interface / 客户端接口 | `api` 只是其中一个 adapter 的名字 |
| boundary | seam | 与 DDD 的 bounded context 撞车 |
| status / state | 二选一 | `status` 是消息名与通道名，`state` 是那五个值 |
| layout 字符串 | 布局树（`Layout` / `LayoutNode`） | 存的是树 |
| Maximize | zoom | 与 macOS 绿点最大化不是一回事 |
