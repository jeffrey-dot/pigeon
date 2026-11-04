# pigeon 远端（SSH）方案

> 目标：本地窗口可以接管**服务器上**的终端；服务器上跑同一个 `pigeon-server`，
> 关掉本地窗口任务继续跑，重连原样恢复。全部走系统 `ssh`，不开端口、不管密码。

> **2026-11-04 核对**：以下客户端流程已在 macOS 接入；Linux 当前仅连接本地宿主。历史实跑证据见 [m4-checklist.md](m4-checklist.md)。

## 1. 三种对接方式（按可用性从高到低）

| 方式 | 做法 | 需要远端装 pigeon | 能持久化 | 备注 |
|---|---|---|---|---|
| **A. `--proxy` 桥接（推荐）** | `ssh host pigeon-server --proxy`：把 stdin/stdout 上的协议帧转给远端宿主的 unix socket | ✅ | ✅ | 不依赖 sshd 的 stream-local 转发开关（很多机器默认关闭） |
| B. unix socket 直转 | `ssh -N -L /tmp/p.sock:<远端 --socket-path 返回值> host` | ✅ | ✅ | 简洁，但需要远端 `AllowStreamLocalForwarding yes` |
| C. 退化模式 | 普通 `ssh host` 开一个 pty | ❌ | ❌（会话随 ssh 断开） | **仍然有状态显示**：我们就是终端，OSC 7501 会直接到达；只是没有远端持久化与远端分屏 |

三种都能同时支持：连接时先探测（A → B → C），并明确告诉用户当前用的是哪一种。

`--stdio` 是同一条传输的另一种用法：不桥接已有 socket，而是让本次启动的 `pigeon-server` 直接作为宿主在
stdin/stdout 上说话。当前客户端优先使用 `--proxy`；它可在没有宿主时自动拉起独立宿主，再桥接 socket。
两种模式的帧格式与 `docs/protocol.md` 完全一致。

## 2. 只往远端推一个很小的宿主（不是整个 pigeon）

两个产物，职责分开：

| 产物 | 包含 | 大小目标 |
|---|---|---|
| `Pigeon.app`（客户端） | GUI、渲染、字体、输入法、侧栏 | 30–50 MB（本机） |
| **`pigeon-server`**（会话宿主） | PTY + 内核（VT / 布局 / OSC 7501）+ socket 协议；**无 UI、无字体、无渲染** | **静态单文件 3–8 MB** |

`pigeon-server` 就是“最小 tmux”：它只负责“让进程活着 + 记住屏幕 + 说话”。本机和远端跑的是**同一份源码按目标平台构建的宿主**，
所以本机与远端行为一致，内核也只有一个实现。

### 自动安装（VSCode Remote 的做法，但不用包管理器）

首次连接某个主机时，客户端自己在 ssh 上完成安装，不需要 root、不需要 PATH、不改 rc：

1. `ssh host 'uname -sm'` → 确定目标平台（完整架构矩阵见下表）；兼容宿主探测失败后才从本机缓存或 Release 取产物；
2. 对 `~/.pigeon/server/` 下每个已装的 `pigeon-server` 跑 `--version`（输出 `pigeon-server <版本> proto <大>.<小>`），
   取最新的、**协议大版本与客户端相同**的那个直接用。发行版本号不同、协议小版本不同都不重装（小版本协商目标与当前差异见 `docs/protocol.md` §0/§3.1）；**不算哈希，也不需要本机有这个文件**，所以已经装好的机器连接时不下载；
3. 一个都没有、都跑不起来，或协议大版本都不同（不兼容），才去取本机的产物，把二进制**从同一条 ssh 连接流过去**（不依赖 scp/ftp）：

   ```sh
   ssh host 'd=~/.pigeon/server/0.1.0; mkdir -p "$d" \
     && tmp=$(mktemp "$d/.pigeon-server.XXXXXX") \
     && cat > "$tmp" && chmod 755 "$tmp" \
     && mv -f "$tmp" "$d/pigeon-server"' \
     < pigeon-server
   ```

   临时文件名由 `mktemp` 生成（Linux 与 macOS 同一写法），两个窗口同时安装时各写各的，
   谁也不会在另一方检查完、`mv` 之前把它的文件截断；任何一步失败都删掉临时文件。

4. 在远端执行一次临时文件的 `--version`：输出不是期望的那一行（传输被截断、架构不对）就删文件并报错，通过了才 `mv` 上线；
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

远端没有兼容的 server、本机缓存里也没有对应平台的产物时，客户端（`ServerDownload.swift`）才下载：取 `releases/download/v<App 版本>/SHA256SUMS` →
下载对应 `.gz` → **校验哈希后**才解压、`chmod +x`、改名进 `~/.cache/pigeon/server/<版本>/`；
校验不过什么都不留。同一版本同一平台只下一次。`PIGEON_SERVER_URL` 可指向镜像（内网 / 访问 GitHub 困难的地区），
但必须是 `https`（含重定向后的最终地址）；明文 `http` 只允许 `localhost` / `127.0.0.1` / `::1`（本机测试用）。
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

`clients/macos/Sources/PigeonApp/Net/Host/HostInstaller.swift`：`ssh uname -sm` 探测平台 → 列出远端已装的 server 并读 `--version` →
有协议大版本相同的（`pg_proto_version()`）就**不传**，否则才向本机要产物，放进以 App 版本号（`pg_core_version()`）命名的目录，把二进制流进
`~/.pigeon/server/<版本>/.pigeon-server.XXXXXX`（`mktemp`），**在远端**跑通 `--version` 后才 `mv` 成正式文件，
并只保留最新两个版本。远端的登录 shell 可能是 fish/csh，所以命令行走 base64 + `sh`，
不假设是 POSIX sh。实跑记录见 `docs/m4-checklist.md`。

### 规则

- **安装在远端用户的家目录**：`$HOME/.pigeon/`（用 `$HOME` 展开，不假设家目录路径），
  目录结构：`~/.pigeon/server/<version>/pigeon-server`、`~/.pigeon/state/`（socket、日志）。
  绝不写 `/usr/local`、不用 sudo、不改 PATH 与 rc。
- **只在不兼容时升级**：App 升级但协议大版本没变，远端继续用已装的 server；大版本变了才推新目录（`server/0.2.0/`），
  老会话继续跑老版本直到重启；保留最近 2 个版本并 GC。
- **启动走 `--detach`**（与本机一致）：问 socket 有没有宿主在答；没有才在独立进程组里起一个，
  等它监听后返回。已在服务的宿主不论版本都保留，不替换（替换就等于结束用户的会话）。
- **已知缺口：大版本升级无法交接。** 老宿主占着 `~/.pigeon/state/pigeon.sock`，新二进制的 `--detach` / `--proxy`
  都会连到它，`hello` 因大版本不同被拒，客户端只能提示。宿主没有 pidfile，`shutdown` 又必须在 `hello` 成功之后才收，
  所以新二进制没有办法让旧宿主退出。PTY 进程属于旧宿主，换宿主必然结束这些会话，只能由用户确认。
  建议的最小方案（需改协议，未实现）：把 `shutdown` 定为**跨大版本冻结**的消息，`hello_error{reason:"version"}`
  之后仍接受；客户端遇到版本不符时问“结束远端 N 个旧会话并升级？”，确认后发 `shutdown`，再用新二进制 `--detach`。
- **安装控制的现状**：安装层提供 `allowInstall` 参数，当前 GUI 连接默认允许按需安装；首次安装确认、“以后不再问”和 `--no-remote-install` 用户入口尚未实现。
- **卸载**：`rm -rf ~/.pigeon` 就是卸载，远端不留任何其它痕迹。
- **静态链接**（musl / 自带依赖）：不依赖远端 glibc、openssl、字体；只依赖内核 + PTY。
- **体积是硬指标**：CI 里卡 8 MB，超了就把可选能力拆成编译 feature。
- **跑不了就退化**：架构不支持、家目录是 `noexec` 挂载、磁盘满、无写权限 → 自动落到方式 C（普通 ssh），并说明原因；
  `--remote-dir <path>` 用户入口尚未实现，当前不能用该参数更换安装位置。

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
- **反向**（字段以 `docs/protocol.md` §3 为准）：`keys{pane_id, bytes}`（macOS 鼠标事件也编码后走这里）、
  `resize_pane{pane_id, cols, rows}`、`new_session{id, cwd?}`、`close_pane{id, pane_id}`、`focus{pane_id}`。
- **保活**：帧级心跳（15s）；ssh 断开即视为可重连，重连走一次新的 `hello + attach`。
- **压缩**：zstd 只有协议预留，宿主当前不提供此能力；尚无压缩路径。

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

- 侧栏按机器、项目分层；项目只显示组名，`host / 组名` 是内部持久化标识。Machines 行显示 connected/connecting/offline。
- 每个主机的会话归到其用户分组：组名在会话**第一次出现时**定下（git 根优先，否则当时 cwd 的末级；
  远端 cwd 取宿主上报），之后 `cd` 不会把会话换组；重命名只改组名。
- 断线：客户端显示连接失败/恢复提示并重试，离线主机不产生在线会话行；同一宿主重连可通过 `attach.since` 续传，缓冲不足时复原快照。
- 一个主机一条 `ssh` 连接（ControlMaster 复用，见 §6），N 个会话共用。

## 5. 安全

- 只经 `ssh`；`pigeon-server` **只监听 unix socket**，权限 0700：装在 `~/.pigeon/server/` 下的远端宿主用
  `~/.pigeon/state/pigeon.sock`（`$XDG_RUNTIME_DIR` 会在用户最后一个登录会话结束时被 systemd 删掉，宿主就成了孤儿）；
  其它情况（本机、开发构建）用 `$XDG_RUNTIME_DIR/pigeon.sock`，回退 `/tmp/pigeon-<uid>/pigeon.sock`（与 tmux 的
  `/tmp/tmux-<uid>/` 同理：目录 0700 且属于当前用户，别人事先建好的同名目录会被拒绝，不会被沿用）。
  宿主与客户端都用 `lstat` 校验目录与 socket 的属主和类型，连上后再比对端 uid（详见 `docs/protocol.md` §1）。
  同一路径只允许一个宿主：绑定前先对 `<socket>.lock` 加 `flock`。
- `--proxy` 只转发协议帧：不接受任意命令、不做 shell 展开。
- **已知缺口：release 没有签名。** `SHA256SUMS` 与二进制来自同一来源，只能发现下载损坏，挡不住来源本身被篡改
  （GitHub 账号或镜像被攻破）；目前唯一的保护是 https。要补上需要一把由维护者持有的签名密钥（例如 minisign /
  sigstore），客户端内置公钥、校验 `SHA256SUMS` 的签名后再信任其中的哈希。
- 用户填的主机名只当 ssh 目的地：以 `-` 开头（如 `-oProxyCommand=…`，会在本机执行命令）、为空或含空白/控制字符的
  直接拒绝，不降级成普通 ssh；每次调用 ssh 都在目的地前加 `--`。拼进远端 shell 的值里，远端返回的二进制路径一律单引号转义。
- 不修改远端 shell 配置；安装层的 `allowInstall` 可阻止安装，但 GUI 确认与命令行安装控制尚未接入，不能承诺当前会先弹确认。
- 认证/密钥/known_hosts 全部交给系统 `ssh`；pigeon 不读、不缓存任何凭据。
- 远端进程以普通用户身份运行，与用户手敲命令同权限。

## 6. ssh 连接细节

- 复用：实现使用 §3 所列 `/tmp/pigeon-<uid>-ssh/%C` 与 `ControlPersist=120`；不改用户的 SSH 配置文件。
- 不做交互式密码：认证失败的提示里给出"先在终端里 ssh 一次"的指引。
- 跳板机：完全交给 `~/.ssh/config`（`ProxyJump`），pigeon 不解析。
- `--detach --log PATH` 可指定宿主追加日志；当前 `--proxy` 自动拉起宿主时未传日志路径，输出丢弃，尚无默认远端日志轮转。

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
