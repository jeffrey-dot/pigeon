# Linux 历史搜索

2026-11-05 核对。复用 [文本查询](text-query.md) 和 [同代次定位](search-progress.md) 的核心接口；不在 GTK 解析 VT、CBOR 或自行重建历史文本。

## 操作

- 聚焦 pane，`Ctrl+Shift+F` 或顶部放大镜打开紧凑搜索浮层；浮层不改变网格尺寸，关闭后恢复 Harp 卡片布局。
- literal 搜索当前 pane 的全部保留历史与屏幕，备用屏只搜索当前备用屏；`Match ASCII Case` 关闭时仅折叠 ASCII 大小写。
- 输入合并 150ms，再分批请求。每批最多 128 个命中，结果列表最多 10,000 个；到达上限明确提示 refine，不声称精确总数。
- 首个命中自动定位，Return/↓ 前往下一项，Shift+Return/↑ 前往上一项。高亮只属于核心返回的同 generation 画面；跨屏范围仅显示当前一屏。
- Stop 同时取消分页与未完成定位；保留已返回的部分结果。Search Again 重新从头查询；过期时不自动无限重试。
- Escape/Close 关闭并回到实时屏；点击终端后键入、IME、粘贴、回看操作也结束搜索。搜索输入不会进入 PTY。
- 缺少 `text_query` 或 `text_viewport` 时显示不可用，禁用输入，不向旧宿主发送查询。

## 画面和身份

搜索绑定当前 Workspace 连接、pane 与会话选择代次，每个请求另有 id。换 pane/会话、断线、编辑、取消会丢弃旧请求；延迟编辑回调也校验同一 pane/选择。
当前客户端没有自动重连，连接替换会建立新 Workspace；此机制不替代未来重连所需的 instance/connection generation 检查。

`Screen` 分别保存实时屏和可选不可变搜索画面。实时 snapshot/delta 仍维护实时基准，但不覆盖搜索画面；新输出令搜索过期。
关闭后请求完整实时快照，收到之前继续保护旧搜索画面。搜索画面不显示实时光标，不发送格子鼠标/链接动作，不授权状态确认；复制只含当前可见画面。
高亮由核心行/列/byte 范围映射到格子，组合码点所在格与宽字符续格包含在内；字体整形继续使用同一网格。

## 验证

```sh
cargo test -p pigeon-linux --locked
cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings
cargo build -p pigeon-linux --locked
python3 -B tools/checks/linux-search-check.py
PIGEON_TEST_NO_SEARCH=1 python3 -B tools/checks/linux-search-check.py
```

测试脚本使用独立 Xvfb/HOME/socket 与真实 PTY，透明 recorder 通过安全 Rust API 编解码；延迟查询答复 150ms。
覆盖历史中的中文/组合文本、同代次画面、下一命中、Escape/实时输入恢复、晚到答复、输出过期、跨会话取消及旧能力退化。
`PIGEON_SEARCH_SCREENSHOT=/tmp/search.png` 保存实际 GTK 定位高亮截图。普通输入/IME、侧栏确认、回看、现代键鼠脚本均已复跑通过。

本机 Arch Linux/GTK 4.24.1：GTK 84 passed / 2 ignored，fmt/clippy/build 通过；默认忽略项仍为手动整形基准和另跑的 GL driver 检查。
本次未改核心协议/ABI。托管 Ubuntu CI 单独核对；真实 Wayland、其它 IME、硬件 GPU、SSH UI 未验。
regex、完整 Unicode 折叠、稳定锚点、跨历史选择与反向宿主扫描仍未完成；持续输出可能反复使查询过期。
