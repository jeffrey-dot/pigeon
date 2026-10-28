# VT parity

同一段字节流喂两个实现——我们的内核（`core/examples/vt-dump`）与参考终端（tmux
`capture-pane`）——逐行比较屏幕文本。内核自称「跨平台 VT」的唯一凭据就是这套对账。

```sh
tools/vt-parity/run.sh            # 跑全部语料，逐条 ok/FAIL
tools/vt-parity/run.sh --record   # 用 tmux 重新生成 golden（需要 tmux）
```

- `corpus/*.vt`：原始字节（可含任意转义序列），入 `corpus/` 即被自动发现。
- `golden/*.txt`：参考终端对同一段字节渲染出来的屏幕；`--record` 才会改它。
- 尺寸默认 80×24，可用 `COLS=132 ROWS=43 tools/vt-parity/run.sh` 覆盖。

## 新增一例

1. 把字节写进 `corpus/<名字>.vt`（用 `printf`/Python 写二进制，别用编辑器补转义）。
2. `tools/vt-parity/run.sh --record` 生成 `golden/<名字>.txt`。
3. 再跑一次 `tools/vt-parity/run.sh`：如果新语料 FAIL，先判断是内核错了还是**语料本身
   依赖了 tmux 的偏好**（例如 tmux 的 `capture-pane` 会把连续空格压成 tab，`run.sh`
   已经用 `expand` 抹平）。内核错就修内核并补一条 `core/src/term/` 的用例。

`golden/` 是参考终端的结果，不是我们的结果：改内核后如果 golden 对不上，说明行为变了，
要么修回去，要么承认差异并写进 `docs/plan.md` 的已知缺口。

## 归一化规则

两侧都做：行尾空白去掉、末尾空行去掉、tab 按 8 列展开。其余（中间的空白行、宽字符的位置）
必须逐字一致。宽字符的第二格是 continuation，dump 侧按 `width == 0` 跳过。

## 现在覆盖了什么

`basic`（CR/LF、tab、无末尾换行）、`cursor`（CUP/CUU/CUD/CUF）、`erase`（EL0/EL1/EL2）、
`wide`（CJK 宽字符与框线）、`scroll-region`（DECSTBM 滚动 + LF）、`alt-screen`（1049 进出）、
`insert-delete`（IL/DL/ICH/DCH）、`wrap`（80 列自动换行）。

未覆盖：SGR 属性（对账只比文本、不比颜色/属性位）、字符集（SO/SI）、鼠标上报、
OSC（7501/133/52 等）、vi 模式。它们各自有 `core/tests` 的确定性用例。
