# M4 验收记录（SSH 远端）

> 规则同前：能自动化的用测试卡住，只能看的用截图或手测；没验到的写“未验”。
> 本轮所有远端数字都来自一台真实的 Arch 主机（`ssh arch`，`Linux x86_64`，登录 shell 是 **fish**）。

## 1. 自动化

```sh
cargo fmt --check                                  # 通过
cargo clippy --all-targets -- -D warnings          # 通过
cargo test                                         # 80 单元 + 11 ABI + 30 宿主端到端 = 121 passed
cargo build --release --bin pigeon-host            # 1,350,240 B（1.29 MiB ≤ 8 MiB，macOS arm64）
cargo build --lib --no-default-features            # 通过（库本体不引用 tokio/portable-pty）
cbindgen --config core/cbindgen.toml --crate pigeon-core --output /tmp/pg-m4.h core   # 与 include/pigeon_core.h 无差异
tools/vt-parity/run.sh                             # 8/8 ok（M2 的对账，M4 没动内核渲染）
swift build && swift test                          # 103 executed / 5 skipped / 0 failures（远端用例默认 skip）
PIGEON_SSH_HOST=arch PIGEON_HOST_ARTIFACTS=$PWD/core/target/x86_64-unknown-linux-musl/release \
  swift test                                       # 103 passed / 0 skipped，exit 0（含 5 条真 ssh 用例）
```

交叉产物（docs/remote.md §2）：

```sh
rustup target add x86_64-unknown-linux-musl
cargo build --release --target x86_64-unknown-linux-musl \
  --config "target.x86_64-unknown-linux-musl.linker=$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/lib/rustlib/aarch64-apple-darwin/bin/rust-lld" \
  --manifest-path core/Cargo.toml --bin pigeon-host
# → core/target/x86_64-unknown-linux-musl/release/pigeon-host  1,744,848 B  static-pie stripped
```

新增覆盖：

| 面 | 用例 |
|---|---|
| `--stdio` | `stdio_mode_serves_a_client_and_keeps_the_socket_for_later`：stdio 客户端建的会话，socket 客户端也能看到（同一进程两种入口） |
| `--proxy` | `proxy_mode_bridges_stdio_to_a_running_host`：经代理跑 `echo $((6*7))`，真宿主回 `42` |
| `--socket-path` | 客户端问二进制要默认 socket 路径，`ssh -L` 不再复刻这条规则 |
| 窗口自检（`PIGEON_SELFTEST=1`） | 应用自己驱动菜单 action（`⌘D`/`⌘⇧↩`）、第一响应者 PageUp、侧栏 `Machines` 行选中；宿主日志确认收到 `<- split_pane`/`<- zoom_pane`/`<- unzoom`/`<- scroll`，见 `docs/m2-checklist.md` §2 |
| zoom（客户端） | `ZoomTests`（`⌘⇧↩` 的 keyEquivalent/modifier/selector、`zoomRequest` → `zoom_pane`/`unzoom`、`hides(pane:zoom:)`）——上一轮审计指出的“zoom UI 客户端没有测试”已补 |
| 多主机侧栏 | 每台已知主机启动即连接（实测 `perHost=local:1,arch:1`、`groups=~\|arch / jeffrey`），会话按 `host / project` 分组，机器名就在分组标题上；离线主机整组收起（不产生会话行），正在连接的主机标题带 `· connecting`；加机器走 `Hosts` 菜单（勾选、`Use This Mac`、`Connect to Host…` ⌘⇧O），`HostsStoreTests` 3 条 + `SidebarOrderingTests` 的分组标题用例 |
| 自动安装 | `HostInstallerTests` 10 条：平台↔产物匹配（绝不把 Linux x86_64 推向别的平台）、远端脚本先校验再 `mv`、缺 `sha256sum` 时退回 `shasum`、内容和哈希目录、二次连接不再上传、拒绝安装、未知平台、远端哈希不符；**1 条真 ssh E2E**（见下） |
| 远端断线重连（窗口） | `HostRecovery.plan(hasRemote:)` + `canRetryRemote`（20 次 × 1.5s ≈ 30s，远超本地 3×1s）；`PigeonApp.recover` 在远端模式以 `connect(allowDegraded: false)` 重新探测——**失败就重试，绝不悄悄退化成不持久的 C**；`RemoteSessionTests.testDegradingCanBeRefusedOutright` 卡住这条 |
| 传输探测 | `RemoteSessionTests` 9 条：A→B→C 顺序、无宿主时退化并说明成本、A 用本地 `pigeon-host --stdio` 冒充远端跑通握手、C 的本地 VT 两条（直接喂字节 / 子进程管道 + 回调）、**3 条真 ssh**（bridge / 退回 ssh -L / 退化）+ **1 条 30s 断连恢复** |
| 多主机侧栏 | `SidebarOrderingTests`：`groupLabel(host:project:)`、同一项目在两台机器上分属两组 |

## 2. 远端实跑（`ssh arch`，Linux x86_64，fish 登录 shell）

**自动安装**（`PIGEON_SSH_HOST=arch swift test --filter HostInstallerTests`）：

```text
installed at /home/jeffrey/.pigeon/host/<hash>/pigeon-host (version <hash>)
Executed 10 tests, with 0 failures
```

（版本目录名每次都是当次产物的内容哈希；实跑过的几次：`0fbc4c2b5d1ea8c6`、`317a3f57c95e71ee`、`906acb77a14fe912`，
远端始终只留最新两个，下面是当时 `ls` 的输出，数字会随产物变化。）

远端落点（`ssh arch 'sh -c "ls -1 ~/.pigeon; ls -1 ~/.pigeon/host; ..."'`）：

```text
host
---
<newest>
<previous>
---
-rwxr-xr-x 1723313  /home/jeffrey/.pigeon/host/0fbc4c2b5d1ea8c6/pigeon-host
-rwxr-xr-x 1723312  /home/jeffrey/.pigeon/host/49237f4a1d316461/pigeon-host
---
usage: pigeon-host [--socket PATH] [--shell PATH] [--stdio|--proxy]
```

- 首连传输、二次命中（`sent=false`，不再上传）；E2E 用“产物 + 随机后缀”算哈希，所以连跑两次都成立
  （两次实跑：`installed at …/317a3f57c95e71ee/…`、`installed at …/906acb77a14fe912/…`，均 passed；远端仍是 2 个版本目录）
- 篡改期望哈希：远端 `pigeon-host: hash mismatch`，tmp 与空版本目录都被删掉，`~/.pigeon` 下只剩 `host`
- 只保留最新两版（第三版出现时旧的被 GC）
- 登录 shell 是 fish，所以脚本走 `base64 | sh`，不假设 POSIX：这是第一次跑挂掉的原因（`fish: 缺少结尾以平衡 if statement`）

**传输探测**（`PIGEON_SSH_HOST=arch swift test --filter RemoteSessionTests`）：

```text
testRealSshBridgeCarriesTheProtocol ... passed (5.2s)        # A: --proxy 桥，host 回 sessions
testRealSshFallsBackToTheForwardedSocket ... passed (11.0s)  # A 被拒 → B: ssh -L 转发 socket
testRealSshWithoutAHostDegradesAndSaysWhatItCosts ... passed # C: plain ssh 起来，note 明说不持久
```

- 退化证据用的是“拒绝 `--proxy`”而不是真的远端 `AllowStreamLocalForwarding no`：这台主机的 sshd 配置我改不了，
  但客户端看到的现象一样（A 失败 → 试 B → 还不行才 C）。
- C 模式另有一条手工记录（xctest 里驱动 ssh pty 的管道在该环境不稳定，见 §4）：

  ```sh
  printf 'uname -s\nexit\n' | ssh -o BatchMode=yes -tt arch
  # … fish 提示符 / bracketed paste / OSC 133 序列 … 最后一行是 Linux
  ```

**断网 30s 恢复**（`PIGEON_SSH_HOST=arch swift test --filter testRealSshDrop`）：

```text
testRealSshDropKeepsTheRemoteSessionAndItsScreen ... passed (40.0s)
```

用例流程：`--proxy` 建会话 → `seq 1 200`（超过 24 行，覆盖屏幕 + 滚动历史）→ 杀掉 ssh 通道（宿主眼里的断网）
→ `sleep 30` → 重新探测并 `attach` → **200 条输出行与断开前逐行相等**。远端 pane 与内容都在。

## 3. 这次抓到的真 bug

| 问题 | 表现 | 位置 |
|---|---|---|
| tokio 的 stdout 是带缓冲的 | `--stdio` 客户端发出 hello 后永远等不到 `hello_ack`（帧停在 writer 里） | `core/src/bin/pigeon-host.rs` 的 `send()`：`write_all` 后补 `flush` |
| 远端登录 shell 不一定是 POSIX sh | 第一次安装直接死在 fish 的语法错误上 | `HostInstaller.sshRunner`：命令行走 `base64 \| sh`，脚本自己 `trap rm` |
| 被拒的传输会留一个空版本目录 | 篡改哈希失败后 `~/.pigeon/host/<v>/` 还在 | 安装脚本失败分支 `rmdir` |
| `HostClient` 只认一个 fd | `ssh --proxy` 的读/写是两个管道，接不上 | `HostClient(read:write:)` + `RemoteSession.pipeEnds` |
| 后台宿主占住 ssh 通道 | 远端 daemon 的 stdin 还是 ssh 的 pty，后续会话可能拿不到通道 | `nohup <bin> </dev/null >/dev/null 2>&1 &` |

## 4. 未验 / 已知缺口

| 项 | 自动化到哪一步 | 还缺什么 |
|---|---|---|
| GUI 侧栏的多主机分组 | 纯逻辑单测（`groupLabel` + 分组拆分） | 真开 `PIGEON_REMOTE=arch pigeon-product` 看分组与截图；本轮没有 GUI 手测，checklist 里没有截图 |
| 连接状态展示 | 退化时窗口标题 `pigeon — arch (not persistent)` + 头部徽标 `Not persistent`；A/B 无额外标识 | 侧栏行上区分“本机 / 远端 / 已断”的视觉 |
| GUI 多主机与重连的*观感* | `PIGEON_REMOTE=arch` 的侧栏分组由脚本验证（`groups=arch / jeffrey`）；`PIGEON_SELFTEST` 步 5 用 `NSTableView.selectRowIndexes` 选中会话行，走真实 delegate → `onSelect` | 物理鼠标点击本身（本会话投递不进窗口；`tools/click-at-pid.swift` 留给正常桌面会话） |
| 退化模式（C）的窗口尺寸 | 本地 VT 会 resize | 远端 pty 尺寸仍是 ssh 分配的 80×24（客户端没有本地 pty 去转发 `TIOCSWINSZ`），见 plan §11 |
| C 模式的 xctest 渲染断言 | 本地 VT 两条单测 + 手工 ssh 记录 | xctest 里把 ssh 的 pty 管道喂给 `LocalSession` 时读线程 25s 拿不到数据（本地子进程正常），原因未定位；不愿为它造假 |
| 真的关掉 `AllowStreamLocalForwarding` 的主机 | 用拒绝 `--proxy` 模拟 A 失败 | 一台能改 sshd 配置的主机 |
| EPIPE 把测试进程打死 | 退化模式的 ssh 子进程先死时，写它的 stdin 会触发 `SIGPIPE`，`swift test` 以 `signal 13` / exit 1 结束 | `LocalSession` 忽略 `SIGPIPE` 并改用 `try? write(contentsOf:)`；现在全量 ssh 用例 `exit 0` |
| 远端大文件/慢网 | 1.7MB 单文件流式传输 | 200ms 延迟下的体感、断点续传（没有做，失败了重传） |
| `zstd` 帧内 tag | 只有定义 | 远端实测后决定阈值 |

## 5. CI

本轮改动未推 CI。推上去后应看：`rust` 作业（`cargo test` 119，含 `--stdio`/`--proxy` 两条真实 PTY 用例）与
`swift` 作业（103 executed / 5 skipped；远端用例需要 `PIGEON_SSH_HOST`，CI 上跳过是预期行为）。
交叉产物（musl）不进 CI：`swift` 作业只需要 macOS 的 host 二进制；需要时按 docs/remote.md §2 的命令单独构建。
