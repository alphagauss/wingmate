# Android 本地运行载荷

从 Wingmate 根目录运行 `pnpm runtime:android`。MJS 入口在 Linux 上直接调用构建工具，在 Windows 上使用默认 WSL 发行版。构建环境需要 Bash、Python 3.11+、curl、xz、tar 和常用 Linux 工具。Python 3.11 至 3.13 还需 `zstd` 解包 Ubuntu deb。脚本自行下载锁定的 Linux Node 构建 DSH 依赖和 File Viewer。下载缓存位于 `work/cache/android`，交付给 Gradle 的构建输入是 `work/build/android/runtime-arm64-v8a.zip`。其中的清单记录 XZ 归档与 PRoot 原生库的校验值，构建报告记录裁剪原因和解包占用。首次准备后，手机可直接从本地 APK 释放环境。

Linux 基础包、Node、Python 和 PRoot 按 [dsh-Go 的 288234ae](https://github.com/ccch1mneyyy/dsh-Go/tree/288234ae52fdfcfc2b0a0388763c371d0b09ea7d) 移植。保留 Ubuntu 的 apt/dpkg、Git、时区和证书，保留 Node 的 npm、pnpm 与 Python 的 pip、venv。文档、缓存、Python 测试和 GUI 组件按上游规则裁剪。Ubuntu deb 锁从该基础包信任的签名索引生成。PRoot 二进制采用 Termux 包及其构建配方，重命名为 Android APK 可携带的原生库并修正 talloc 动态库名。

从 dsh-Go 移植的构建脚本及辅助代码沿用 AGPLv3，包括 [`build-runtime.sh`](build-runtime.sh) 和 [`lib/rtbuild.py`](lib/rtbuild.py)。载荷中各第三方软件包适用各自的许可证。[dsh-Go 许可文本](LICENSE.dsh-go)

DSH 使用 Wingmate 的 npm 锁安装 Linux ARM64 glibc 依赖，复用 [Office 裁剪与预览替换](../../plugins/wingmate-file-viewer/README.md#上游适配)。手机本地管理由 Kotlin 模块提供。生成目录、参考仓库和开发机器上的 npm 安装树均独立于载荷输入。

本机工作台的 HTML 在 DSH 启动脚本前加载 WebView 兼容代码，补齐旧版 WebView 缺少的会话连接和工作区配置接口。升级 DSH 时，核对 HTML 注入位置和上游 API 用法。旧版 WebView 退出支持范围后可移除此补丁。

本机工作台通过回环地址连接 DSH。Android 没有默认网络时，WebView 会报告离线，回环地址仍然可用。Android 载荷对回环页面使用实际的连接状态。升级 DSH 时，复验手机离线下的自动连接；上游能独立处理回环连接后可移除此补丁。

DSH 在 Android 来宾的 `/root/Documents` 下创建默认工作区。来宾没有桌面环境的 XDG 目录查询工具，因此通过 DSH 的 Documents 路径配置指定位置。该目录随来宾用户数据保留。

资源包的安装、启停与构建入口见 [Wingmate Android](../../apps/mobile/README.md)。
