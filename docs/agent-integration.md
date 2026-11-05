# Agent 主动上报接入

2026-11-05 核对。`pigeon-report` 是用户可直接运行的 OSC 7501 发送工具，
`pigeon-run` 是可选的前台命令 wrapper。只依赖 **Python 3.8+ 标准库**；wrapper 另需 POSIX `/bin/sh`。
macOS 不保证预装 Python 3，请先确认 `python3 --version`；Linux 也需自行提供 Python 3。
工具不安装 hook，不修改 shell/agent 配置，不分析屏幕、标题、日志，不投递桌面或外部消息。

## 安装与第一条记录

在仓库根执行；安装目标只在用户目录，无需 sudo：

```sh
python3 --version
mkdir -p "$HOME/.local/bin"
install -m 0755 tools/report/pigeon-report "$HOME/.local/bin/pigeon-report"
install -m 0755 tools/report/pigeon-run "$HOME/.local/bin/pigeon-run"
```

两文件须在同一目录。可以直接使用完整路径，或在当前 shell 主动执行
`export PATH="$HOME/.local/bin:$PATH"`；工具不把它写入启动配置。
也可从源码直接运行 `tools/report/pigeon-report`。当前应用包、远端宿主包没有自动附带或安装这些脚本；
在远端 pane 使用时，把脚本安装到远端用户目录并提供 Python 3。

在 Pigeon pane 里主动运行：

```sh
pigeon-report working --id demo.task --app 'My agent' --msg '正在检查: core = ready'
pigeon-report blocked --id demo.task --app 'My agent' --kind permission --msg 'Allow build?'
pigeon-report working --id demo.task --app 'My agent' --msg 'Building'
pigeon-report done --id demo.task --app 'My agent' --msg 'Build complete'
```

在 Status Records 查看并确认终态。当前 pane 聚焦时客户端可能已按其查看规则确认记录；
需要检查逐条未确认集合时可在后台 pane 上报。完整集合与客户端行为见
[状态同步](status-sync.md)、[状态查看与确认](status-client.md)。上报 `blocked` 表示程序确实需要人处理；
普通网络等待、运行中的工具不应报 `blocked`。程序恢复后主动用同 id 报 `working`。

## 命令接口与输出去向

```text
pigeon-report STATE [--id ID] [--kind KIND] [--msg TEXT] [--app TEXT]
              [--stdout] [--dry-run]
pigeon-report clear (--id ID | --all) [--stdout] [--dry-run]
pigeon-run ID APP -- COMMAND [ARG ...]
```

| 参数 | 当前工具行为 |
|---|---|
| `STATE` | `idle` / `working` / `blocked` / `done` / `error`，以及清除命令 `clear` |
| `--id` | 非空 ASCII 段，段内仅字母、数字、`_`、`-`，以 `.` 连接；如 `shell_1.agent-2.tool_3`。省略时使用 pane 默认记录；显式空字符串被拒绝 |
| `--kind` | 仅 `blocked` 可带：`permission` / `question` / `auth`；也可以省略 |
| `--msg` | 原始 UTF-8 纯文本，工具做标准 base64 编码；不要先编码。不支持换行、制表或其它控制字符 |
| `--app` | UTF-8 明文名称，可含空格、等号，不可含冒号或控制字符；**不做 base64** |
| `clear --id a` | 删除 `a` 及所有 `a.` 开头的子记录，不影响 `ab` |
| `clear --all` | 清空当前 pane 所有记录，包括其它程序及未确认终态；仅在明确需要时手工使用 |
| `--stdout` | 明确将 OSC 原始字节写到 stdout，允许文件或管道；调用者负责把字节送到正确 pane |
| `--dry-run` | 校验全部参数与长度，只打印脱敏 JSON；不发 OSC，不读取 TTY 输入 |

默认打开 `/dev/tty`，因此 `pigeon-report working >log` 仍写入当前控制终端，日志不混入 OSC。
没有可写控制 TTY 时退出 2，提示显式 `--stdout`；即使环境有 `PI_PROGRAM_STATUS=1` 也不回退到日志。
默认写 TTY 但没有能力提示时会在 stderr 警告支持未确认；有提示也不能证明接收端已记录。
不做 `state=?` 探测，因为当前核心忽略它且不返回支持应答。

退出码：0 表示整条字节已写出（或 dry-run 校验通过），**不是已接入/已展示/已确认**；
2 表示参数、长度或无 TTY 错误；1 表示写失败，可能已有不完整字节写出。
验证在写入之前完成，非法参数不会产生半条记录；不会截断用户文本后假装成功。
每次发送为一次写调用，但与其它进程同时向同一个 PTY 写数据仍无跨进程原子性保证。

显式管道可用于开发验证，例如只查看十六进制字节，不上报到侧栏：

```sh
pigeon-report blocked --kind question --msg 'Choose: A or B?' --stdout | od -An -tx1
```

文本会出现在进程参数和接收端记录中；仅传需要给用户看的消息，不把凭据放进 `msg` 或 `app`。
本工具的诊断和错误不会回显参数值，也不枚举环境或读取凭据。

## 实际支持的 OSC 7501 子集

核对依据为 [`core/src/status/parse.rs`](../core/src/status/parse.rs)、
[`StatusStore`](../core/src/status/mod.rs)、[ADR-0005](adr/0005-status-only-from-osc-7501.md)。
**仓库没有钉住上游 OSC 7501 revision 编号，也未实现 revision 协商**；这里描述的是本仓库
2026-11-05 的已实现子集，不能宣称支持某个上游 revision 的全部能力。
`status_snapshot.revision` 是宿主的集合修订号，不是 OSC 规范版本。

工具发送以下形式（`ESC` / `ST` 为控制字节，不是这些字母）：

```text
ESC ] 7501 ; state=blocked:id=shell.task:kind=question:app=My agent:msg=Q2hvb3NlPw== ST
```

| 项目 | 核心接收语义与工具约束 |
|---|---|
| 分隔与终止 | `key=value`，字段以 `:` 分隔；核心接受 BEL 或 `ESC \` 结尾，工具固定使用 `ESC \`。无额外换行 |
| 状态字段 | 核心认识五种状态与 `clear`；未知状态、缺少状态、`state=?` 不创建记录 |
| 文本编码 | `msg` 标准 base64，解码后须为 UTF-8；核心也接受缺少末尾 padding 的合法 base64，工具总带 padding。`app`、`id`、`kind` 不解码 |
| 控制字符 | 原始字段和解码后的 msg 都拒绝 C0（U+0000–001F）、DEL（U+007F）、C1（U+0080–009F）；整条丢弃，不保留部分字段 |
| 长度 | 核心最多 **4096 字节整条 OSC**，包括 `ESC ]7501;`、所有字段、分隔符和终止符。按 UTF-8 字节计，不按字符数；base64 膨胀计入上限 |
| 工具长度校验 | 单个原始文本、带 framing 的原始报告、base64 后的完整报告都不超过 4096 字节；最终可用 msg 大小随 id/app/kind 变化，无统一“4096 字符消息”额度 |
| id 与树 | 核心没有额外的 ASCII id 语法检查，仅受字段分隔、UTF-8、控制字符和整条长度约束；清除以精确 id 和 `id.` 前缀匹配。工具采用上表更严格的非空点分段约束，避免注入和意外树形 |
| 默认记录 | 核心缺少/空 id 时更新默认记录。核心无 id 的 `clear` 清空全部；工具要求显式 `--all`，不能用空 id 只清默认记录 |
| kind | 核心将不认识的 kind 映射为空；工具拒绝未知 kind，且仅允许 blocked 使用 |
| 未知字段 | 核心忽略未知 key，不代表支持它；原始字段仍不能含控制字符。工具不提供任意字段透传 |
| 可选字段更新 | 同 id 每次报告替换 state/kind/msg/app；没带的字段变为空，不继承。相同 state 保持进入状态的计时，但记录 seq 仍更新 |
| 容量 | 每 pane 最多 256 条记录；满时可回收已确认记录，否则拒绝新 id，已有 id 仍可更新。发送工具拿不到接收结果，不能报告容量已足够 |

支持字段仅 `state`、`kind`、`msg`、`app`、`id`。工具不支持结构化 progress、超链接、任务动作、超时、
pid 生命周期绑定等未来字段，也不发送 CBOR 或宿主控制消息。
客户端协议及独立状态快照的文本预算见 [protocol.md](protocol.md)，不要混淆 OSC 字节上限与 wire 限制。

## 生命周期与 id 所有权

- 一个程序为自己的一次运行分配唯一根 id，同一次状态变化更新它；独立请求或子工具用不同子 id。
  id 仅在 pane 内区分记录，没有身份认证。重复使用 id 会替换旧的未确认 `done/error`，因此不要跨运行复用固定 id。
- `done/error` 保留到用户确认，或被程序显式替换/clear；wrapper 不在退出后自动 clear 终态。
  `clear` 是程序主动撤回子树，不等于用户确认，也不会写入客户端“已看过”身份。
- pane 的直接子进程（通常 shell）退出，或收到 OSC 133 `A` / `A;...` 提示符时，
  核心删除所有非终态记录（`idle/working/blocked`），保留 `done/error`。其它 OSC 133 标记不触发清除。
  提示符清除覆盖整个 pane，即使还有后台任务；后台任务须主动再报，不能假定提示符只清当前前台 id。
- 宿主不知道 shell 内任意 agent/子工具何时退出。没有显式上报或 OSC 133A，杀掉 agent 后可能留下旧 live 记录。
  SIGKILL 不能被 trap；机器/宿主退出也不会产生“完成”。宿主状态仅在内存中。

## 用户主动运行的 wrapper / trap

[`pigeon-run`](../tools/report/pigeon-run) 是可审查、可执行的 POSIX shell trap 示例，**不要 source**。
可从 bash/zsh/sh 调用；它不替换当前交互 shell，也不读取用户 rc。传入的程序是否读取自身配置由用户选择。

```sh
run_id="shell-$$.run-$(python3 -c 'import uuid; print(uuid.uuid4().hex)')"
pigeon-run "$run_id" Build -- make test
result=$?
printf 'Command exit: %s\n' "$result"
```

wrapper 先 dry-run 校验最坏终态长度，再发 `working`，两步成功才启动命令；命令在独立前台子 shell 执行，
stdin 和参数逐项原样传递，`exec` / `trap` 等 builtin 不能替换 wrapper 或移除其退出处理。
id/app 以 `--id=…` / `--app=…` 传给发送器，合法的 `-run` 或 `--help` 文本不会被当作选项。
命令退出 0 发 `done`，任何非零退出（含通常的 128+signal）发 `error`，并保留命令退出码。
HUP/INT/TERM trap 将退出码设为 129/130/143，并阻止退出 trap 误报 done；最终上报失败也不覆盖命令退出码。
它只记录所包的前台命令，不跟踪该命令遗留的后台进程或整个 agent 的内部任务。

终端 Ctrl-C 通常发给整个前台进程组。示例针对这种使用；若只向 wrapper PID 发信号，
POSIX shell 可能等待前台命令结束后才执行 trap，**不会负责转发到所有后代进程**。
外部取消应面向实际前台进程组。SIGKILL 没有终态上报，应依赖后续真实 prompt/显式 clear；
不要为所有退出路径无条件安装 `trap 'pigeon-report done' EXIT`。

嵌套时 wrapper 把当前根 id 导出为 `PIGEON_REPORT_ID`。子工具用它派生自己的 id，
不能把阻塞工具的退出写成父 agent 的退出。下面是用户放入自己脚本的一个步骤：

```sh
# Executed inside a command launched by pigeon-run.
step_id="${PIGEON_REPORT_ID}.compile"
pigeon-run "$step_id" Compiler -- make all
step_result=$?
# The parent still owns its root record and decides what happens next.
pigeon-report working --id "$PIGEON_REPORT_ID" --app 'My agent' --msg 'Reviewing build result'
```

此例子工具失败只产生 `*.compile` 的 error；父记录继续 working，最后由父命令的退出 trap 决定终态。
同一个父运行中多次编译也应为每次独立结果使用不同子 id。对于权限/问题/认证，
应在程序确实提出请求的 hook 中调用 `blocked --kind ...`，响应后调用 `working`。
通用 wrapper 只知道进程起止，不能自动发现这些中间状态。

## 支持矩阵与诊断

| 接入方式/环境 | 当前结果 | 能证明什么 |
|---|---|---|
| 原生上报 | 程序自己向 Pigeon pane 输出上述子集，核心消费，客户端按协商显示 | 需要验证具体程序版本、开启选项与实际报告；本任务未验第三方 agent |
| `PI_PROGRAM_STATUS=1` | Pigeon 为新 pane 注入的能力提示，部分程序可能识别 | **不是自动接入保证**，不是鉴权、接收回执或程序正在上报的证明 |
| 用户显式接入 | 手工运行工具、wrapper，或自己在明确生命周期 hook 调用 | CLI 可发五种状态和 clear；wrapper 自动发起止，blocked 仍需程序主动上报 |
| 未上报程序 | 未监控，侧栏不凭屏幕/标题生成状态和活动时间 | 能运行该程序不代表能跟踪其状态 |
| 其它终端的 TTY | 工具能写字节；没有提示时警告，接收支持未知 | 退出 0 不代表该终端支持或显示 OSC 7501 |
| 无控制 TTY（CI、后台 service、纯管道） | 默认报错；显式 `--stdout` 仅输出字节 | 不会把日志文件视为已监控 pane |
| SSH | 必须让报告进入目标 pane 的 PTY；远端须自行安装工具/运行依赖 | 不自动转发环境，不通过 SSH 安装脚本；具体连接能力见 [remote.md](remote.md) |
| tmux/其它中间层 | 可能吞掉或改变 OSC，当前工具不配置穿透 | 不保证端到端，不能只看环境提示判断已接入 |

先做无副作用诊断：

```sh
pigeon-report blocked --kind question --id diagnose --msg 'Ready?' --dry-run
```

JSON 只含校验结果、原始/编码字节数、目标类型、TTY 可用性、能力提示布尔值、
`emitted:false` 与 `receiver_verified:false`。没有 msg、app、id、路径、完整环境或凭据。
dry-run 即使没 TTY 也可成功校验；随后默认实际发送仍会因没 TTY 失败。
若记录不可见，依次检查实际目的 pane、TTY、字段/长度、报告是否被后续同 id 更新或 prompt 清除、
是否被用户确认、256 条容量和中间层；不要把探测或 CLI 成功当作接收确认。

## 验证范围

```sh
python3 tools/checks/report-check.py
cargo fmt --all -- --check
rustfmt --check tools/checks/helpers/report-probe.rs
cargo clippy --all-targets -- -D warnings
cargo test --manifest-path core/Cargo.toml --offline
git diff --check
```

脚本在隔离 HOME/配置目录和真实 PTY 中运行 CLI、wrapper，用临时 Cargo 包依赖
`pigeon-core`（`default-features = false`），仅经 `pigeon_core::api` 解析并读取记录；
不在 Python 中重写 OSC parser。辅助程序也执行 clippy。需要先缓存仓库依赖，测试本身 `--offline`。
覆盖 Unicode、编码和原始长度、畸形输入、clear 树、prompt、TTY/重定向、无 TTY、诊断脱敏、
命令参数/stdin、嵌套 id、退出码及 HUP/INT/TERM 取消（包括子命令信号后退出 0）。
2026-11-05 Linux / Python 3.14.9 实跑：17 组工具测试通过（另含 SIGKILL、安装副本末次上报失败），
core 全量 315 passed、2 ignored；fmt、辅助程序 rustfmt、clippy、文档链接与 `git diff --check` 通过。
本次 Linux CLI/API/PTY 验证不代表 macOS 实机、双平台 GUI、第三方 agent、远端链路或桌面通知已验收。

主线程复审后的 21 组测试通过：新增选项样式字段、builtin 隔离、启动前终态预算和 dry-run 写失败退出码回归，
四组先失败后修复。诊断通过受控写调用输出，写失败返回 1，不依赖解释器退出时刷新。CI 在 macOS/Ubuntu 的结果由集中执行记录维护。
