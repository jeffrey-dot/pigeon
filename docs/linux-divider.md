# Linux 分隔线拖动

2026-11-05 核对。GtkGLArea 内的鼠标命中只使用 `api::layout_splits` 给出的 split id、轴、ratio 和所在矩形；客户端将像素坐标映射成比例，`ResizeSplit` 仍由宿主/核心计算布局和最小格数。

- 未 zoom 的分隔线两侧约 5 个逻辑像素内显示调整光标；左键拖动左右或上下分隔线可改变比例，嵌套分隔线按各自矩形计算。
- 拖动全程消费鼠标按下/移动/释放，不向 pane 内鼠标应用泄漏事件，也不触发 OSC 8 点击。拖动开始取消未完成的链接/按键手势及 IME preedit。
- 移动请求最多每 16ms 一次，释放提交最终比例；画面只接受宿主返回的布局，不在 GTK 先算另一套 pane 几何。窗口外拖动比例限制为 0–1，实际最小 pane 尺寸由核心钳制。
- zoom 后没有命中区域；会话变化、失焦和连接重置取消拖动；树中 split 消失或所属矩形变化时停止发送旧手势请求。
- 可见分隔线现在按 `rect.origin + rect.size × ratio` 绘制，和命中位置相同；单纯绘制外框不代表可拖动分隔线。

## 验证

```sh
cargo test -p pigeon-linux --locked
cargo clippy -p pigeon-linux --all-targets --locked -- -D warnings
cargo build -p pigeon-linux --locked
python3 -B tools/checks/linux-divider-check.py
```

纯逻辑用例覆盖左右/上下、嵌套矩形、zoom、区域外/NaN/零尺寸、越界比例；真实 Xvfb/PTY 启用应用鼠标上报后拖动分隔线，验证请求确实到达宿主、两个 pane 及嵌套三 pane 调整、没有应用鼠标字节、zoom 禁用和极端比例仍正常。
本机 GTK 97 passed / 2 ignored，fmt/clippy/build 通过；现代键鼠、侧栏、搜索、重连回归单独复跑。真实 Wayland、混合 DPI 与硬件指针尚未验收，不以 Xvfb 代替。
