# 2026-11-12 Android 本地运行版首包

用 Windows JDK 17、Android SDK 36 和 Gradle Wrapper 构建两个 Release APK，在 WSL Ubuntu x86_64 通过 `pnpm runtime:android` 生成 ARM64 载荷。WSL 中的构建 Node 由脚本准备。dsh-Go 原来锁定的部分 Ubuntu 软件包和 PRoot 二进制已从镜像下架，本次从 Ubuntu 签名索引更新 deb 锁，从 Termux 当前索引和源码配方更新 PRoot 输入。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,493,938 | `d127d0ce773147fedb5c6fa2ac482e2cdebe0f1f6dccaf9c93930a9012514593` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,295,626 | `daa41a73bb6fadd30647851553c2e7411daad15bc67f9ea0190ce421694d315a` |
| 本地版内置 XZ | 131,596,404 | 见载荷 manifest |
| 运行时展开文件合计 | 688,329,944 | 不含文件系统分配开销 |

Office 裁剪排除了 153,158,148 字节的展开文件。本次未做同参数的裁剪前压缩对照。构建报告按来源记录展开占用。

两包通过 v2、v3 签名和 APK 对齐检查。本机新生成的 Debug 证书用于签名，覆盖安装早期远程 APK 需要匹配原签名。远程版成品没有本地运行服务、原生库或运行载荷。本地版的四个 PRoot 原生库和 XZ 与清单哈希一致，归档内 835 个 ELF 通过 ARM64 和 16 KiB 段对齐检查。

Windows 上 JavaScript 测试通过 8 项，Kotlin JVM 测试通过 29 项，3 项需要 POSIX 文件系统的用例跳过。WSL 通过 QEMU 从最终载荷运行 ARM64 Node、Python、Git 和 pnpm，验证 Python 的 SSL、SQLite、venv 和 pip，加载 ARM64 Sharp，使用启动令牌登录 DSH Web，并逐字节核对服务返回的 DOCX、PPTX 预览 Worker。

构建时未连接 Android 真机。首次释放、PRoot 与前台服务、实际模型和工具调用、切换连接、停机恢复以及升级后数据保留仍需在目标设备上验收。

## 工作台连接和默认工作区修复

**本地工作台已通过真机验证的最低组合为 Android 12 和 Android System WebView 96.0.4664.104（ARM64）。** 此次真机验证覆盖这一组合下的本机 Agent 启动、工作台连接和默认工作区自动创建。

通过无线 ADB 复现问题。工作台的 WebSocket 握手返回 101，前端因缺少 Promise 和取消接口反复重连。连接恢复后，默认工作区初始化返回 `spawn xdg-user-dir ENOENT`。本地载荷在 DSH 启动前加载兼容脚本，并通过 DSH 配置指定来宾 Documents 目录。

用新载荷覆盖安装 Debug 包后，真机自动创建默认工作区，显示工作区、会话和输入框，未再出现重连和默认工作区错误。来宾 `/root` 和 `/root/.dsh` 的 inode 保持一致，`/opt/dsh` 已替换。WSL 的隔离空数据目录也通过了登录、兼容脚本加载顺序和默认工作区创建验证。此次未调用真实模型。

本地 Debug 与签名 Release APK 内的 XZ 哈希一致。更新后的本地 Release 为 133,283,338 字节，SHA-256 为 `ef92809d9f71d84c9b194c12f80dfa39c02a86a5e0a48b01d959e996d45ce9e4`。远程版重新构建并通过签名、对齐和包内容检查。
