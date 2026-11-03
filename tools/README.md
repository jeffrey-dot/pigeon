# tools/

开发期脚本，不进产品。每个脚本自己定位仓库根，从任何目录运行都可以。

| 脚本 | 做什么 |
|---|---|
| `build/bundle-app.sh` | 打 Pigeon.app；`UNIVERSAL=1` 出双架构（发行用） |
| `bench/render-bench.sh` | 渲染基准：大文件 `cat` + 5 秒持续输出，读 `PIGEON_FRAME_STATS` |
| `checks/m0-gui-check.sh` | M0：单 pane、关窗重开（对应 [m0-checklist](../docs/m0-checklist.md)） |
| `checks/m1-gui-check.sh` | M1：分屏、拖动分隔线、焦点（[m1-checklist](../docs/m1-checklist.md)） |
| `checks/m2-gui-check.sh` | M2：宿主崩溃恢复、多客户端尺寸、真键盘（[m2-checklist](../docs/m2-checklist.md)） |
| `checks/m3-gui-check.sh` | M3：侧栏、状态、未读（[m3-checklist](../docs/m3-checklist.md)） |
| `checks/harp-ui-check.py --out /tmp/pigeon-ui` | 临时宿主与真实 OSC 会话，截取 Harp 风格浅色 / 深色主界面 |
| `checks/metal-gui-check.sh` | Metal 渲染：两种外观各截一张 |
| `checks/quit-on-last-session-check.sh` | 最后一个会话退出后宿主结束、应用退出 |
| `checks/helpers/` | 上面脚本用的 Swift 小工具：按 pid 点窗口、取窗口号 |
| `vt-parity/run.sh` | VT 对账：同一段字节喂内核与参考终端，逐行比较（见 [README](vt-parity/README.md)） |
