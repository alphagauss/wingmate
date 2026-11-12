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
