# macOS 搜索与同代次定位

2026-11-05 核对。S3 的 macOS 搜索切片，基于 [文本查询](text-query.md)；完整 S3（Linux 搜索、regex、跨历史选择等）尚未完成。当前实现已落地，Linux 上核心/宿主和 Foundation/真实 C ABI 可验证；AppKit 类型检查与 GUI 待 macOS 验证，不能按语法解析或核心测试宣称通过。

## 操作

- 聚焦要查找的 pane，按 `⌘F`（Edit → Find in Pane…）。窗口底部显示两行紧凑搜索条，搜索框取得焦点；不向 PTY 发送搜索输入。
- 输入 literal 文本，200ms 合并连续编辑。范围为该 pane 保留的全部历史与当前屏幕；备用屏只含其当前屏幕。`Match ASCII Case` 开启时精确匹配，关闭时仅折叠 ASCII A–Z；没有 regex 模式。
- 一次请求最多 128 个命中，逐页扫描；空页有 next 时仍继续。显示 `Scanning…`、当前命中/数量、`Complete` 或错误。客户端最多保存 10,000 个范围，达到上限显示 `10000+` 和 refine 提示，不声称精确总数。
- 首个命中自动定位。Return/↓ 按钮/搜索框 Down 选下一命中，Shift-Return/↑ 按钮/Up 选上一命中，在已收到的命中内循环。跨多屏范围只高亮返回这一屏的部分。
- `Stop` 停止分页、取消未完成定位；保留已返回的部分范围，可继续导航。编辑/重查/关闭会清除请求 id；晚到页和定位答复被丢弃。
- `Escape`（搜索框或 pane）/`Close` 关闭，焦点回到该 pane，并发既有 `scroll{offset:0}` 请求实时画面。点击 pane 后正常输入/粘贴或滚动也关闭搜索并回实时路径。普通 Tab 保留 AppKit 控件焦点循环；搜索框的复制粘贴是文本编辑行为。
- 输出或 resize 使搜索显示 `Expired — Search Again`，清掉可导航的旧坐标。已显示的查询画面可以继续留在屏幕上，但标记为冻结，不混入后续 delta；`Search Again` 按当前模式从头查询，关闭则回实时屏。
- 切换 pane、会话、host、连接或宿主 instance 后取消搜索。旧宿主缺少 `text_query` 或 `text_viewport` 时显示不可用，不发送新的查询 kind。SSH A/B 与本地使用同一路径；C 档退化模式暂不提供搜索。

## 核心与接口

新增能力 `text_viewport` 与 `TextQuery::Viewport {generation,range}`，请求/答复复用 `text_query` 的 id 和 pane_id；不改变旧 info/search/range。宿主必须同时协商两个能力才接受 viewport。

pane actor 在一次串行调用内校验非零 generation、端点 UTF-8 边界，然后读取对应一屏。feed（包括尚未发出的 delta/同步输出）、resize、淘汰、alt 切换使旧请求返回 `stale_text`。结果包括同代次 info/range 与有限画面，没有全历史、没有实时 seq。定位只读，不改变其它客户端或本连接旧 scroll offset。

Rust 提供 `text_viewport_snapshot` / `text_viewport_of`；C 提供 `pg_text_result_snapshot` / `pg_text_result_viewport`，kind=3，`pg_text_result_hit(0)` 读取同画面范围。生成头文件使用 cbindgen 0.27。Swift 只映射接口类型，不解析 VT/CBOR。

## 客户端隔离与画面约束

`TerminalSearch.Context` 固定 `(host, instance, connection UUID, pane)`；每页与每次定位各保存 request id。新模式、取消、过期、导航替换均使旧请求失效。命中范围和画面必须同时通过 context/id/generation/range 检查才应用。

定位后清除普通 delta seq 基准和当前连接的实时屏幕证明，冻结该 pane 画面；实时帧仍更新输入 modes，但不改查询画面。搜索高亮与选择高亮分开；宽字符续格包含在主格高亮内，组合码点的 byte 偏移映射到包含它的格子。冻结画面不显示实时光标、不发送历史格子的鼠标报告、不用于 agent 状态已查看/确认。关闭时使用普通 scroll snapshot 重新建立实时基准。

`Search/TerminalSearch.swift` 是 Foundation 纯逻辑；`SearchWire.swift` 为真实 ABI adapter；`SearchBarView.swift` 负责 AppKit 控件；`AppDelegate+Search.swift` 装配请求和显示。没有新增依赖，也没有改 Linux 客户端或 OSC 7501 工具。

## 未完成与限制

regex、完整 Unicode case folding、稳定锚点、宿主反向扫描、跨历史拖选/自动滚动/矩形选择、Linux 搜索 UI 未实现。持续输出会让查询频繁过期，用户需暂停程序或重查；不会自动不断重试。当前匹配导航只覆盖已收到的前 10,000 个范围。超大/密集附加文本画面仍受既有 8 MiB 帧上限限制。

搜索条占用 64pt，会触发 pane resize；真实字体/窄窗口、IME 组合输入、分屏焦点、鼠标/滚轮、SSH 延迟和实际 Metal 高亮均需 macOS GUI 验收。只做 Linux Foundation 和真实 C ABI 测试不等于 `swift build && swift test` 已通过。

## 验证

本机 Arch Linux x86_64，Rust stable，Swift 6.1.2 Linux 工具链；未更改依赖。

- `cargo test --manifest-path core/Cargo.toml`：321 passed / 2 ignored；最后追加只读/跨 pane 隔离用例后，定位专题再次通过 5/5（默认与无默认 feature），合计默认 322 passed。
- `cargo test --manifest-path core/Cargo.toml --no-default-features`：216 passed / 1 ignored；上述追加定位用例单独复跑通过后合计 217 passed。`cargo build --lib --no-default-features` 通过。
- 默认与无默认 feature 的 `cargo clippy --all-targets -- -D warnings` 通过，`cargo fmt --check` 通过。真实 PTY suite 62 passed，新增 viewport 用例验证请求 id、协商拒绝、真实历史画面、跨连接隔离与输出过期。
- Rust 定位专题 5/5，C ABI 文本查询专题 4/4；涵盖 pending output/同步输出、resize、alt、淘汰、Unicode、独立所有权、错误 kind/size、零 generation、畸形 wire 画面和只读隔离。
- `tools/checks/swift-search.sh`：6/6（5 个 Foundation 状态模型/高亮测试、1 个真实 C ABI 端到端内存/请求测试）；`swift-logic.sh` 102/102、`swift-status-wire.sh` 3/3。使用 `SWIFT=/tmp/pigeon-swift-toolchain/usr/bin/swift` 和兼容 ncurses/ICU/XML 库路径；这三项均实际编译并链接，不是语法解析。
- `tools/vt-parity/run.sh`：8 条 golden 全通过；`tools/checks/ffi-imports.sh` 通过。cbindgen 0.27 生成头文件，重新生成至临时路径 diff 一致。
- GNU release 宿主 2,040,032 bytes；musl static-pie 2,158,440 bytes，二者均小于 8 MiB。musl `--version` smoke 通过；真实 PTY 使用 GNU 构建。
- 文档相对链接与 `git diff --check` 通过。新增 AppKit `SearchBarTests` 覆盖导航/Escape 路由与布局不抢搜索焦点，尚未在本机运行；语法解析不代表 AppKit 类型检查。

需父线程在 macOS CI 跑 `swift build && swift test`，并手工验收上述操作。Ubuntu/macOS 核心复验、Apple Silicon、SSH 弱网、长时间负载与真实搜索延迟仍未验收。
