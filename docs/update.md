# 软件更新

Pigeon.app 自己检查、下载、安装新版本，不依赖 Sparkle 或其它框架。代码在 `clients/macos/Sources/PigeonApp/Update/`。

## 用户看到什么

- 启动 10 秒后检查一次，之后每 24 小时检查一次。没有新版本时什么都不显示。
- 有新版本时弹窗：`Pigeon 0.2.0 is available`，按钮 `Install and Relaunch` / `Later`。
  选 Later 后，本次运行期间的自动检查不再问同一个版本。
- 应用菜单里有 `Check for Updates…`，手动检查总会给出结果（已是最新 / 有新版本 / 检查失败）。
- 安装时替换 Pigeon.app 并重启。本机的 `pigeon-server` 是独立进程，会话不受影响，新窗口启动后重新接上。
- 从源码构建（没有 bundle 版本号）的副本不检查更新。

## 发布产物

`.github/workflows/release.yml` 在 `v<版本>` tag 上额外产出：

| 资产 | 内容 |
|---|---|
| `Pigeon-<版本>.zip` | `UNIVERSAL=1 tools/bundle-app.sh` 打的 Pigeon.app（arm64 + x86_64），用 `ditto -c -k --keepParent` 压缩 |
| `latest.json` | `{version, url, sha256, signature}` |

客户端读 `https://github.com/jeffrey-dot/pigeon/releases/latest/download/latest.json`；发布步骤会把该 release
标为 latest。`PIGEON_UPDATE_URL` 可指向镜像上的 `latest.json`。

## 信任

- 签名用 Ed25519。私钥只在仓库 secret `PIGEON_UPDATE_KEY`（PEM）里，公钥写在 `Updater.publicKeyBase64`。
- 被签的消息是 `pigeon-update <版本> <zip 的 sha256>`，所以版本号和压缩包绑在一起，换掉任一个签名都失效。
- 客户端依次检查：`latest.json` 格式完整（版本号合法、哈希是 64 位小写十六进制、URL 是 https）→ 签名有效 →
  版本比当前新（只升不降）→ 下载的 zip 哈希等于签名里的哈希 → 解出来的 bundle 标识是 `dev.pigeon.app`、
  版本等于签名里的版本、可执行文件存在。任何一步不过都不安装。
- 换密钥意味着所有已安装的副本都认不出新版本，只能让用户手动重装，所以私钥要备份好。

## 安装细节

- 新版本解压到与 Pigeon.app **同一卷**上的临时目录（`itemReplacementDirectory`），再用 `replaceItemAt` 换进去，
  中途失败不会留下半个 app。
- 从“下载”里直接打开的 app 会被 macOS 放到只读的随机路径（App Translocation），这时提示用户先移到 Applications。
- 所在目录不可写时，提示用户移到可写的位置。
- 重启：起一个 `/bin/sh` 等当前进程退出，再 `open` 新的 Pigeon.app。

## 发版步骤

1. 改 `core/Cargo.toml` 的 `version`。
2. 打 tag 并推送：`git tag v0.2.0 && git push origin v0.2.0`。
3. 工作流构建所有 `pigeon-server`、Pigeon.app，签名 `latest.json`，发布到 release。

首次配置：把签名私钥存成仓库 secret，`gh secret set PIGEON_UPDATE_KEY < update-key.pem`。

## 已知缺口

- 没有 Developer ID 签名与公证。从网上下载的第一份 Pigeon.app 需要用户在“系统设置 → 隐私与安全性”里放行；
  之后的更新由 app 自己下载，不带隔离属性，不会再拦。
- 没有增量更新，每次下载完整的 zip。
- 没有更新说明，弹窗只显示版本号。
