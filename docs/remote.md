# pigeon 远端（SSH）方案

> 目标：本地窗口可以接管**服务器上**的终端；服务器上跑同一个 `pigeon-host`，
> 关掉本地窗口任务继续跑，重连原样恢复。全部走系统 `ssh`，不开端口、不管密码。

## 1. 三种对接方式（按可用性从高到低）

| 方式 | 做法 | 需要远端装 pigeon | 能持久化 | 备注 |
|---|---|---|---|---|
| **A. `--proxy` 桥接（推荐）** | `ssh host pigeon-host --proxy`：把 stdin/stdout 上的协议帧转给远端宿主的 unix socket | ✅ | ✅ | 不依赖 sshd 的 stream-local 转发开关（很多机器默认关闭） |
| B. unix socket 直转 | `ssh -L /tmp/p.sock:$XDG_RUNTIME_DIR/pigeon.sock host` | ✅ | ✅ | 简洁，但需要远端 `AllowStreamLocalForwarding yes` |
| C. 退化模式 | 普通 `ssh host` 开一个 pty | ❌ | ❌（会话随 ssh 断开） | **仍然有状态显示**：我们就是终端，OSC 7501 会直接到达；只是没有远端持久化与远端分屏 |

三种都能同时支持：连接时先探测（A → B → C），并明确告诉用户当前用的是哪一种。

`--stdio` 是同一条传输的另一种用法：不桥接已有 socket，而是让本次启动的 `pigeon-host` 直接作为宿主在
stdin/stdout 上说话（首次安装后的第一次启动用它）；之后它常驻，后续连接用 `--proxy` 桥接到它的 unix socket。
两种模式的帧格式与 `docs/protocol.md` 完全一致。

## 2. 只往远端推一个很小的宿主（不是整个 pigeon）

两个产物，职责分开：

| 产物 | 包含 | 大小目标 |
|---|---|---|
| `Pigeon.app`（客户端） | GUI、渲染、字体、输入法、侧栏 | 30–50 MB（本机） |
| **`pigeon-host`**（会话宿主） | PTY + 内核（VT / 布局 / OSC 7501）+ socket 协议；**无 UI、无字体、无渲染** | **静态单文件 3–8 MB** |

`pigeon-host` 就是“最小 tmux”：它只负责“让进程活着 + 记住屏幕 + 说话”。本机和远端跑的是**同一个二进制**，
所以本机与远端行为一致，内核也只有一个实现。

### 自动安装（VSCode Remote 的做法，但不用包管理器）

首次连接某个主机时，客户端自己在 ssh 上完成安装，不需要 root、不需要 PATH、不改 rc：

1. `ssh host 'uname -sm'` → 选对应架构的内嵌产物（Linux x86_64/aarch64、macOS arm64/x86_64）；
2. 检查 `~/.pigeon/host/<version>/pigeon-host` 是否存在且哈希匹配；
3. 缺失就把二进制**从同一条 ssh 连接流过去**（不依赖 scp/ftp）：

   ```sh
   ssh host 'mkdir -p ~/.pigeon/host/3.2.0 \
     && cat > ~/.pigeon/host/3.2.0/pigeon-host.tmp \
     && chmod +x ~/.pigeon/host/3.2.0/pigeon-host.tmp \
     && mv ~/.pigeon/host/3.2.0/pigeon-host.tmp ~/.pigeon/host/3.2.0/pigeon-host' \
     < pigeon-host
   ```

4. 校验 sha256（客户端内嵌期望值，远端 `shasum -a 256` 比对；不一致就删文件并报错）；
5. 启动：`ssh host ~/.pigeon/host/3.2.0/pigeon-host --proxy`（宿主已在远端常驻，桥接它的 unix socket；首次安装后用 `--stdio` 拉起常驻实例，两者都能被同一客户端复用）。

### 规则

- **安装在远端用户的家目录**：`$HOME/.pigeon/`（用 `$HOME` 展开，不假设家目录路径），
  目录结构：`~/.pigeon/host/<version>/pigeon-host`、`~/.pigeon/state/`（socket、日志）。
  绝不写 `/usr/local`、不用 sudo、不改 PATH 与 rc。
- **版本目录并存**：升级只推新目录（`host/3.2.1/`），老会话继续跑老版本直到重启；保留最近 2 个版本并 GC。
- **先问一次**：首次连接弹确认（“将在 host 上安装 pigeon-host 4.1 MB 到 `~/.pigeon/`”），可勾选“以后不再问”；
  `--no-remote-install` 全局关闭。
- **卸载**：`rm -rf ~/.pigeon` 就是卸载，远端不留任何其它痕迹。
- **静态链接**（musl / 自带依赖）：不依赖远端 glibc、openssl、字体；只依赖内核 + PTY。
- **体积是硬指标**：CI 里卡 8 MB，超了就把可选能力拆成编译 feature。
- **跑不了就退化**：架构不支持、家目录是 `noexec` 挂载、磁盘满、无写权限 → 自动落到方式 C（普通 ssh），并说明原因；
  `--remote-dir <path>` 可以换安装位置（例如 `$XDG_RUNTIME_DIR/pigeon` 或 `/tmp/$USER/pigeon`）。

## 3. 连接与协议

```text
客户端                                   远端
  │ ssh host pigeon-host --proxy           │
  ├──────── stdio（长度前缀帧）─────────────►│── unix socket ──► pigeon-host
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

## 4. 多主机与侧栏

- 侧栏第一层是**主机**：`本机` / `host-a` / `host-b`，组头显示连接状态（`connecting` / `offline` / 版本不匹配）。
- 每个主机的会话归到其下的项目分组（与本地同样的分组规则，cwd 来自远端）。
- 断线：该主机整组变灰并标注 `reconnect`，点一下重连；重连成功即复原快照。
- 一个主机一条 `ssh` 连接（ControlMaster 复用，见 §6），N 个会话共用。

## 5. 安全

- 只经 `ssh`；`pigeon-host` **只监听 unix socket**（`$XDG_RUNTIME_DIR/pigeon.sock`，回退 `/tmp/pigeon-<uid>.sock`，权限 0700）。
- `--proxy` 只转发协议帧：不接受任意命令、不做 shell 展开。
- `--remote-install` / `--no-remote-install` 永不静默安装；打印完整命令与哈希；不修改远端 shell 配置。
- 认证/密钥/known_hosts 全部交给系统 `ssh`；pigeon 不读、不缓存任何凭据。
- 远端进程以普通用户身份运行，与用户手敲命令同权限。

## 6. ssh 连接细节

- 复用：优先 `ControlMaster auto` + `ControlPath ~/.ssh/cm-%r@%h:%p`（用户已有配置则沿用）；没有则用一个临时 master。
- 不做交互式密码：认证失败的提示里给出"先在终端里 ssh 一次"的指引。
- 跳板机：完全交给 `~/.ssh/config`（`ProxyJump`），pigeon 不解析。
- 远端 `pigeon-host` 的日志写 `~/.local/state/pigeon/log`（滚动），便于远程排障；`--log` 可覆盖。

## 7. 里程碑对应

| 里程碑 | 内容 |
|---|---|
| M2 | 协议 + 快照（本机后台进程），`hello`/`features` 先定好，避免远端返工 |
| M4 | 宿主自动推送与安装、`--proxy` / socket 直转 / 退化模式、多主机侧栏分组与重连 |
| M5 | Linux 客户端复用同一协议；远端也可是 Linux |

## 8. 验收

- 断网 30s 后恢复：远端任务不受影响，重连后屏幕与滚动一致。
- 远端无 pigeon：自动退化到方式 C，并说明"该主机的会话不会持久化"。
- 版本不匹配：拒绝连接 + 给命令，不静默尝试。
- 200ms 延迟下：按键回显 ≤1 帧的额外延迟（不做本地预测）。
- `kill -9` 远端 `pigeon-host`：客户端显示"远端会话已丢失"，可重开新会话。
