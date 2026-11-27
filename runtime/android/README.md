# Android 本地运行载荷

从 Wingmate 根目录运行 `pnpm runtime:android`。MJS 入口在 Linux 上直接调用构建工具，在 Windows 上使用默认 WSL 发行版。构建环境需要 Bash、Python 3.11+、curl、xz、tar 和常用 Linux 工具。Python 3.11 至 3.13 还需 `zstd` 解包 Ubuntu deb。脚本自行下载锁定的 Linux Node 构建 DSH 依赖和预装插件。下载缓存位于 `work/cache/android`，交付给 Gradle 的构建输入是 `work/build/android/runtime-arm64-v8a.zip`。其中的清单记录 XZ 归档与 PRoot 原生库的校验值，构建报告记录裁剪原因和解包占用。首次准备后，手机可直接从本地 APK 释放环境。

Linux 基础包、Node、Python 和 PRoot 按 [dsh-Go 的 288234ae](https://github.com/ccch1mneyyy/dsh-Go/tree/288234ae52fdfcfc2b0a0388763c371d0b09ea7d) 移植。保留 Ubuntu 的 apt/dpkg、Git、时区和证书，保留 Node 的 npm、pnpm 与 Python 的 pip、venv。文档、缓存、Python 测试和 GUI 组件按上游规则裁剪。PRoot 二进制采用 Termux 包及其构建配方，重命名为 Android APK 可携带的原生库并修正 talloc 动态库名。

新安装的 Python 裁掉静态链接 Berkeley DB 的 `_dbm` 扩展。新建 `dbm` 数据库可使用 `dbm.dumb`，已有 `dbm.ndbm` 数据库需在带原扩展的环境中读取。Ubuntu 的 PAM、Perl 和 SASL 模块仍依赖系统 `libdb5.3`，该库继续随包提供。

从 dsh-Go 移植的构建脚本及辅助代码沿用 AGPLv3，包括 [`build-runtime.sh`](build-runtime.sh) 和 [`lib/rtbuild.py`](lib/rtbuild.py)。载荷中各第三方软件包适用各自的许可证。[dsh-Go 许可文本](LICENSE.dsh-go)

DSH 使用 Wingmate 的 npm 锁安装 Linux ARM64 glibc 依赖，复用 [Office 裁剪与预览替换](../../bundled-plugins/wingmate-file-viewer/README.md#上游适配)。手机本地管理由 Kotlin 模块提供。生成目录、参考仓库和开发机器上的 npm 安装树均独立于载荷输入。

本机工作台通过回环地址连接 DSH。Android 没有默认网络时，WebView 会报告离线，回环地址仍然可用。Android 载荷对回环页面使用实际的连接状态。升级 DSH 时，复验手机离线下的自动连接；上游能独立处理回环连接后可移除此补丁。

来宾没有桌面环境的 XDG 目录查询工具，因此 DSH 的 Documents 路径配置使用 PRoot 的共享工作区挂载点。目录选择器的起点通过 Android 载荷补丁设在同一挂载点。上游支持配置起点后，可用配置替换补丁。两处共享目录的用途及手动备份见 [Wingmate Android](../../apps/mobile/README.md#工程布局)。

Android 共享存储由系统管理文件的访问权限，来宾看到的权限位可能宽于 DSH 凭据组件要求的 `0600`。Android 载荷仅对 `/root/.dsh/.credentials.yaml` 跳过此权限位校验。升级 DSH 时，真机确认写入凭据后重启仍能读取。上游支持显式配置该路径的权限位策略后，可用配置替换补丁。

Android 共享存储可能不支持 `flock` 和硬链接。共享 `.dsh` 内的会话用私有存储上的 `flock` 文件维持跨进程写入互斥，日志在持锁期间通过同目录重命名发布。升级 DSH 时，真机验证创建会话、停止后续接和卸载重装后读取。上游支持独立配置会话锁目录与原子发布方式后，可用配置替换补丁。

共享 `.dsh` 中的附件也通过私有目录中的写锁和同一挂载内的重命名发布。保留原文件名的附件路径需要独立副本。升级 DSH 时，真机验证图片和普通文件能够保存并读回。上游支持在无硬链接的文件系统上存储附件后，可移除此补丁。

资源包的安装、启停与构建入口见 [Wingmate Android](../../apps/mobile/README.md)。

发行流程用 `bash runtime/android/build-runtime.sh <源码输出目录>` 同时准备 Gradle 载荷和源码归档的输入。Ubuntu 源码从实际选入的二进制包版本解析，归档包括其 `.dsc`、源码和补丁。所需文件优先从 Ubuntu Archive 下载，失败时回退到 Launchpad 的对应版本。其余来源见 [随包组件源码说明](SOURCES.md)。

## Ubuntu 系统依赖

本地版随 Ubuntu 基础环境交付 `curl`、`jq`、`zip`、`unzip` 和 `rg`（包名 `ripgrep`），供 DSH 和用户从标准 Shell 路径直接调用。构建按 [Ubuntu ARM64 包锁](locks/ubuntu-noble-arm64.json) 下载和校验 `.deb`，保留 dpkg 记录，首次安装时随 APK 离线释放。

新增或升级随包 Ubuntu 系统依赖时，从 Ubuntu Ports 取得与基础环境匹配的 ARM64 索引，验证各发行套件的 `InRelease` 签名和索引 SHA-256。用 [`rtbuild.py resolve-debs`](lib/rtbuild.py) 重新解析完整的 `want` 清单。参数 `--base` 使用构建脚本锁定的 Ubuntu Base 归档，`--arch` 使用 `arm64`，各索引分别通过 `--packages` 传入。审阅 `--out` 生成的锁文件差异，再构建载荷，验证受影响的命令和对应源码归档。包变更影响提示中的工具限制时，同步更新 [Android 本机环境插件](../../bundled-plugins/wingmate-android-environment/README.md)。

实际随包的 `.deb` 内容参与计算基础环境身份。安装器遇到不同基础环境时保留现有 rootfs 并阻止引擎升级。含系统包变更的发行按新基础环境准备，用户数据位置见 [Android 本机引擎](../../apps/mobile/README.md#工程布局)。
