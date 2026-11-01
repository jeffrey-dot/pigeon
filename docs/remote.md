# pigeon 远端（SSH）方案

> 目标：本地窗口可以接管**服务器上**的终端；服务器上跑同一个 `pigeon-server`，
> 关掉本地窗口任务继续跑，重连原样恢复。全部走系统 `ssh`，不开端口、不管密码。

## 1. 三种对接方式（按可用性从高到低）

| 方式 | 做法 | 需要远端装 pigeon | 能持久化 | 备注 |
|---|---|---|---|---|
| **A. `--proxy` 桥接（推荐）** | `ssh host pigeon-server --proxy`：把 stdin/stdout 上的协议帧转给远端宿主的 unix socket | ✅ | ✅ | 不依赖 sshd 的 stream-local 转发开关（很多机器默认关闭） |
| B. unix socket 直转 | `ssh -L /tmp/p.sock:$XDG_RUNTIME_DIR/pigeon.sock host` | ✅ | ✅ | 简洁，但需要远端 `AllowStreamLocalForwarding yes` |
| C. 退化模式 | 普通 `ssh host` 开一个 pty | ❌ | ❌（会话随 ssh 断开） | **仍然有状态显示**：我们就是终端，OSC 7501 会直接到达；只是没有远端持久化与远端分屏 |

三种都能同时支持：连接时先探测（A → B → C），并明确告诉用户当前用的是哪一种。

`--stdio` 是同一条传输的另一种用法：不桥接已有 socket，而是让本次启动的 `pigeon-server` 直接作为宿主在
stdin/stdout 上说话（首次安装后的第一次启动用它）；之后它常驻，后续连接用 `--proxy` 桥接到它的 unix socket。
两种模式的帧格式与 `docs/protocol.md` 完全一致。

## 2. 只往远端推一个很小的宿主（不是整个 pigeon）

两个产物，职责分开：

| 产物 | 包含 | 大小目标 |
|---|---|---|
| `Pigeon.app`（客户端） | GUI、渲染、字体、输入法、侧栏 | 30–50 MB（本机） |
| **`pigeon-server`**（会话宿主） | PTY + 内核（VT / 布局 / OSC 7501）+ socket 协议；**无 UI、无字体、无渲染** | **静态单文件 3–8 MB** |

`pigeon-server` 就是“最小 tmux”：它只负责“让进程活着 + 记住屏幕 + 说话”。本机和远端跑的是**同一个二进制**，
所以本机与远端行为一致，内核也只有一个实现。

### 自动安装（VSCode Remote 的做法，但不用包管理器）

首次连接某个主机时，客户端自己在 ssh 上完成安装，不需要 root、不需要 PATH、不改 rc：

1. `ssh host 'uname -sm'` → 选对应架构的内嵌产物（Linux x86_64/aarch64、macOS arm64/x86_64）；
2. 跑 `~/.pigeon/server/<版本>/pigeon-server --version`：能执行、输出 `pigeon-server <版本>` 与客户端内核版本一致就直接用，
   **不算哈希，也不需要本机有这个文件**（所以已经装好的机器连接时不下载）；
3. 不存在、跑不起来或版本不对，才去取本机的产物，把二进制**从同一条 ssh 连接流过去**（不依赖 scp/ftp）：

   ```sh
   ssh host 'mkdir -p ~/.pigeon/server/0.1.0 \
     && cat > ~/.pigeon/server/0.1.0/pigeon-server.tmp \
     && chmod +x ~/.pigeon/server/0.1.0/pigeon-server.tmp \
     && mv ~/.pigeon/server/0.1.0/pigeon-server.tmp ~/.pigeon/server/0.1.0/pigeon-server' \
     < pigeon-server
   ```

4. 在远端执行一次 `pigeon-server.tmp --version`：输出不是期望的版本行（传输被截断、架构不对）就删文件并报错，通过了才 `mv` 上线；
5. 启动：`ssh host ~/.pigeon/server/0.1.0/pigeon-server --proxy`（宿主已在远端常驻，桥接它的 unix socket；socket 上没人监听时 `--proxy` 先拉起一个宿主——无共享 stdio、独立进程组，ssh 断开不会带走它——再桥接，所以重启后的第一次连接也只需要这一条命令）。

### 远端产物从哪来（GitHub Release，按需下载）

`Pigeon.app` **不带**任何远端架构的 server，只带本机那一个。远端要的静态产物由
`.github/workflows/release.yml` 在打 `v<版本>` tag 时统一构建，作为 release 资产发布：

| `uname -sm` | 目标 | 资产 |
|---|---|---|
| `Linux x86_64` | `x86_64-unknown-linux-musl` | `pigeon-server-x86_64-unknown-linux-musl.gz` |
| `Linux i686` / `i386` | `i686-unknown-linux-musl` | 同上命名 |
| `Linux aarch64` / `arm64` | `aarch64-unknown-linux-musl` | 〃 |
| `Linux armv7l` / `armv8l` | `armv7-unknown-linux-musleabihf` | 〃（树莓派 2+、多数 ARM 板） |
| `Linux armv6l` | `arm-unknown-linux-musleabihf` | 〃（树莓派 1 / Zero） |
| `Linux riscv64` | `riscv64gc-unknown-linux-musl` | 〃 |
| `Darwin arm64` / `x86_64` | `aarch64-apple-darwin` / `x86_64-apple-darwin` | 〃 |

外加一个 `SHA256SUMS`。Linux 全是 musl 静态链接（纯 Rust，`rust-lld` 直接链，不要交叉 C 工具链），
一个 CPU 一份就覆盖所有发行版。对照表在 `HostInstaller.platforms`，`ServerDownloadTests` 会比对它与工作流矩阵，两边不许漂移。

远端缺这个版本、本机缓存里也没有对应平台的产物时，客户端（`ServerDownload.swift`）才下载：取 `releases/download/v<App 版本>/SHA256SUMS` →
下载对应 `.gz` → **校验哈希后**才解压、`chmod +x`、改名进 `~/.cache/pigeon/server/<版本>/`；
校验不过什么都不留。同一版本同一平台只下一次。`PIGEON_SERVER_URL` 可指向镜像（内网 / 访问 GitHub 困难的地区）。
下载失败时退回普通 ssh 模式（不持久），界面照常说明。

开发构建（没有 bundle 版本号）不下载，直接用 `PIGEON_HOST_ARTIFACTS` 或 `core/target/<triple>/release/pigeon-server`。
本机交叉编一份调试用：

```sh
rustup target add x86_64-unknown-linux-musl
lld=$(ls "$(rustc --print sysroot)"/lib/rustlib/*/bin/rust-lld | head -1)
cargo build --release --manifest-path core/Cargo.toml --bin pigeon-server \
  --target x86_64-unknown-linux-musl --config "target.x86_64-unknown-linux-musl.linker=\"$lld\""
```

### 自动安装怎么做

`Sources/PigeonApp/Net/HostInstaller.swift`：`ssh uname -sm` 探测平台 → 版本目录名就是版本号（`pg_core_version()`）→
远端那个文件能跑且 `--version` 对得上就**不传**，否则才向本机要产物，把二进制流进
`~/.pigeon/server/<版本>/pigeon-server.tmp`，**在远端**跑通 `--version` 后才 `mv` 成正式文件，
并只保留最新两个版本。远端的登录 shell 可能是 fish/csh，所以命令行走 base64 + `sh`，
不假设是 POSIX sh。实跑记录见 `docs/m4-checklist.md`。

### 规则

- **安装在远端用户的家目录**：`$HOME/.pigeon/`（用 `$HOME` 展开，不假设家目录路径），
  目录结构：`~/.pigeon/server/<version>/pigeon-server`、`~/.pigeon/state/`（socket、日志）。
  绝不写 `/usr/local`、不用 sudo、不改 PATH 与 rc。
- **版本目录并存**：升级只推新目录（`server/0.2.0/`），老会话继续跑老版本直到重启；保留最近 2 个版本并 GC。
- **先问一次**：首次连接弹确认（“将在 host 上安装 pigeon-server 4.1 MB 到 `~/.pigeon/`”），可勾选“以后不再问”；
  `--no-remote-install` 全局关闭。
- **卸载**：`rm -rf ~/.pigeon` 就是卸载，远端不留任何其它痕迹。
- **静态链接**（musl / 自带依赖）：不依赖远端 glibc、openssl、字体；只依赖内核 + PTY。
- **体积是硬指标**：CI 里卡 8 MB，超了就把可选能力拆成编译 feature。
- **跑不了就退化**：架构不支持、家目录是 `noexec` 挂载、磁盘满、无写权限 → 自动落到方式 C（普通 ssh），并说明原因；
  `--remote-dir <path>` 可以换安装位置（例如 `$XDG_RUNTIME_DIR/pigeon` 或 `/tmp/$USER/pigeon`）。

## 3. 连接与协议

```text
客户端                                   远端
  │ ssh host pigeon-server --proxy           │
  ├──────── stdio（长度前缀帧）─────────────►│── unix socket ──► pigeon-server
  │                                        │                   ├─ PTY / VT 状态
  └─ 本地渲染 ◄── 快照 + 增量 ─────────────┘                   └─ 布局 / OSC 7501
```

- **握手**：`hello{proto, client, features}` → `hello_ack{proto, server, features, host_id, client_id}`。
  版本不匹配 → `hello_error{reason:"version"}` 并给升级命令；`features` 取交集用于优雅降级
  （例如不支持 `scrollback` 时侧栏隐藏滚动条）。
- **attach**：`attach{id, pane_id}` → 全量快照（含备用屏、光标、滚动区）→ 之后只收脏行增量（`delta`）。
- **反向**（字段以 `docs/protocol.md` §3 为准）：`keys{pane_id, bytes}`、`mouse{pane_id, kind, x, y, button, mods}`、
  `resize_pane{pane_id, cols, rows}`、`new_session{id, cwd?}`、`close_pane{id, pane_id}`、`focus{pane_id}`。
- **保活**：帧级心跳（15s）；ssh 断开即视为可重连，重连走一次新的 `hello + attach`。
- **压缩**：大输出时按帧 zstd（可选能力，握手协商）；默认关，先测延迟。

### ssh 连接复用

一次连接要跑六七次 ssh（`uname`、版本检查、启动宿主、`--proxy`…），每次都完整握手时远端越远越慢（实测 `arch`
每次 ~0.6 s）。客户端给每次调用加 `ControlMaster=auto` / `ControlPath=/tmp/pigeon-<uid>-ssh/%C` /
`ControlPersist=120`：只有第一次握手，其余复用（6 次 4.5 s → 0.5–1.3 s）。只走命令行选项，不碰用户的
`~/.ssh/config`；目录必须属于当前用户且是 0700，否则不复用。`ssh -N -L`（B 档）不复用：经 master 的转发
会挂到 master 上、命令本身立刻退出，进程就不能再代表转发的生死。

## 4. 多主机与侧栏

客户端实现：**每台已知主机启动即各自连一条 ssh**，侧栏分两层：机器标题在上（本机显示
电脑名，远端显示连接时用的 ssh 别名或地址），其下是该机器的项目分组（只显示项目名）；同一项目在两台机器上仍是
两组，改名按 `host / project` 区分、互不影响；离线主机整组收起，正在连接的主机标题带 `· connecting`。
加机器与切主机走侧栏 `+` 下拉菜单或 `Hosts` 菜单（每个已知主机带勾选、`Use This Mac`、`Connect to Host…` ⌘⇧O），
列表与“上次看的是谁”存在 `~/.config/pigeon/hosts.json`（`HostsStore`）。终端区同一时刻只显示一台主机，
点另一台的会话即切换（连接早已建立，不需要重新探测）。

- 侧栏是**一台机器一层组头**：本机组沿用组名，远端是 `host / 组名`；机器离线时整组收起，连接状态见
  `Hosts` 菜单的勾选与窗口提示（不在组头上加临时标记）。
- 每个主机的会话归到其用户分组：组名在会话**第一次出现时**定下（git 根优先，否则当时 cwd 的末级；
  远端 cwd 取宿主上报），之后 `cd` 不会把会话换组；重命名只改组名。
- 断线：该主机整组变灰并标注 `reconnect`，点一下重连；重连成功即复原快照。
- 一个主机一条 `ssh` 连接（ControlMaster 复用，见 §6），N 个会话共用。

## 5. 安全

- 只经 `ssh`；`pigeon-server` **只监听 unix socket**，权限 0700：装在 `~/.pigeon/server/` 下的远端宿主用
  `~/.pigeon/state/pigeon.sock`（`$XDG_RUNTIME_DIR` 会在用户最后一个登录会话结束时被 systemd 删掉，宿主就成了孤儿）；
  其它情况（本机、开发构建）用 `$XDG_RUNTIME_DIR/pigeon.sock`，回退 `/tmp/pigeon-<uid>.sock`。
- `--proxy` 只转发协议帧：不接受任意命令、不做 shell 展开。
- `--remote-install` / `--no-remote-install` 永不静默安装；打印完整命令与版本；不修改远端 shell 配置。
- 认证/密钥/known_hosts 全部交给系统 `ssh`；pigeon 不读、不缓存任何凭据。
- 远端进程以普通用户身份运行，与用户手敲命令同权限。

## 6. ssh 连接细节

- 复用：优先 `ControlMaster auto` + `ControlPath ~/.ssh/cm-%r@%h:%p`（用户已有配置则沿用）；没有则用一个临时 master。
- 不做交互式密码：认证失败的提示里给出"先在终端里 ssh 一次"的指引。
- 跳板机：完全交给 `~/.ssh/config`（`ProxyJump`），pigeon 不解析。
- 远端 `pigeon-server` 的日志写 `~/.local/state/pigeon/log`（滚动），便于远程排障；`--log` 可覆盖。

## 7. 里程碑对应

| 里程碑 | 内容 |
|---|---|
| M2 | 协议 + 快照（本机后台进程），`hello`/`features` 先定好，避免远端返工 |
| M4（已落地） | `--stdio` / `--proxy`、宿主自动推送与安装（按版本号分目录）、A→B→C 探测、多主机侧栏分组；断网 30s 后屏幕与滚动一致。验收与实跑记录见 `docs/m4-checklist.md` |
| M5 | Linux 客户端复用同一协议；远端也可是 Linux |

## 8. 验收

- 断网 30s 后恢复：远端任务不受影响，重连后屏幕与滚动一致。
- 远端无 pigeon：自动退化到方式 C，并说明"该主机的会话不会持久化"。
- 版本不匹配：拒绝连接 + 给命令，不静默尝试。
- 200ms 延迟下：按键回显 ≤1 帧的额外延迟（不做本地预测）。
- `kill -9` 远端 `pigeon-server`：客户端显示"远端会话已丢失"，可重开新会话。
