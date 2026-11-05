# Linux GL 资源边界

核对日期：2026-11-05。客户端的原始 GL、动态库加载和字节上传只位于 `clients/linux/src/render/gl.rs`。
crate 级 `unsafe_code = deny`，只对该模块放行，`undocumented_unsafe_blocks = deny` 要求明确安全依据。

## 所有权与输入检查

- `Gl` 拥有 GDK context 与加载函数所需的 driver libraries；资源通过主线程 `Rc` 保持它们存活，不可跨线程共享。
- `Texture` / `Pipeline` 拥有 GL 名称；释放时临时切到自己的上下文，删除纹理、program、VAO、VBO，再恢复 GTK 原上下文。
  shader 编译/链接失败、后续资源分配失败同样清理已经创建的句柄。
- 纹理上传拒绝非正尺寸、越界、整数溢出及长度与像素格式不符的切片。
- 实例上传仅接受封闭的 `RectInstance` / `GlyphInstance`，两者均为无 padding 的 `repr(C)` float 数组布局；
  不允许任意 Rust 类型被重新解释成字节。
- 字形上传后重新绑定两张图集再绘制。帧开始/结束明确设置或清理 scissor、program 和 texture 绑定。
- GLArea 使用新上下文绘制时，客户端重建属于新上下文的 renderer，不将旧资源名称交给新 context。

资源 metadata 放在 `device.rs` / `texture.rs` / `pipeline.rs`，纯 shader 字符串在 `shaders.rs`；这些文件不包含 unsafe。
驱动入口、GL 调用和原始内存读取集中于 `gl.rs`，字体发现、整形与实例构建不直接调用 driver。

## 验证

`tools/checks/linux-gl-check.sh` 创建独立 Xvfb 软件 GL 环境，显式运行默认忽略的真实 driver 测试：

- 没有当前 context 时拒绝加载；编译错误 shader 返回错误后仍能建立正常 pipeline。
- 32 次纹理/pipeline 创建与释放；在无当前 context 和另一个 context 为 current 时交替清理，恢复先前 context。
- 用 driver 的 `is_texture` / `is_program` / `is_buffer` 验证删除前为真、删除后为假；VAO 随同一资源拥有者删除。
- 验证短数据、越界、负坐标和零纹理尺寸拒绝，不进入非法上传路径。

本机 Arch Linux、GTK 4.24.1、Xvfb/Mesa 软件 GL 通过；fmt、clippy 与 GTK 42 个常规测试通过，2 个默认忽略项分别为手动整形基准和本页单独运行的 GL 测试。
主线程复跑 `linux-input-check.py`、`linux-scroll-check.py` 全部通过；检查隔离桌面截图，历史文字与位置条可见。
CI 的 Ubuntu GTK 作业显式安装 Xvfb/软件 GL 并单独运行资源测试。硬件 GPU、真实 Wayland、上下文丢失恢复和长时间功耗仍需平台验收。
