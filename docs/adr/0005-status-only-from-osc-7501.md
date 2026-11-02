# ADR-0005: 状态只来自 OSC 7501

- 状态：已采纳
- 日期：2026-10-27
- 依据：[plan.md §4](../plan.md)、[`core/src/status/`](../../core/src/status)

## 决定

状态只来自 pane 字节流里的 `ESC ] 7501 ; … ST`，由 `core/src/status/` 解析。
五个值：`idle` / `working` / `blocked` / `done` / `error`。
没有记录时侧栏不显示状态。

新建 pane 时注入 `PI_PROGRAM_STATUS=1`，让程序开口。

## 约束

- 不做屏幕识别、标题正则、agent 专用适配。
- `working` / `blocked` 在进程退出或出现 OSC 133A 提示符时必须清掉；
  `done` / `error` 保留到用户确认（`AGENTS.md` 不变量 4）。
- 没有记录时不拿创建时间冒充相对时间。

## 何时重开

OSC 7501 规范出新版本。
