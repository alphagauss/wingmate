# Wingmate

Wingmate 是一个面向个人使用的 DeepSeek Harness（DSH）发行版。可以在自己的电脑或服务器上运行 Agent，再用浏览器、另一台电脑或 Android 手机接着做任务。

## 亮点

- **轻量单文件**。桌面组件和 DSH 运行时装在一个可执行文件里，使用系统安装的 Node.js。Windows x64 的一次构建约 73 MiB，其中压缩的 DSH 基础运行时约 54 MiB。[体积记录](docs/records/2026-11-13-windows-package-size.md)
- **自带远程访问**。可以用桌面端连接别的 Wingmate 主机，也可以把这台电脑的 DSH 工作台分享给自己的其他设备。支持局域网配对和 Cloudflare 公网隧道。[连接方式](crates/wingmate-service/README.md#web-入口)
- **关窗口，任务照跑**。后台独立运行，可以在界面或命令行查看状态、启动、停止和重启 DSH。服务器可直接使用无界面版本。
- **Android 有两种版本**。纯远程版用来连接已有的 Wingmate 主机。本地版既能远程连接，也能在 ARM64 手机上直接运行 DSH Agent。[Android 使用与构建](apps/mobile/README.md)
- **支持 DSH 和 Codex**。可以在工作台侧栏切换，原来的任务会继续运行。[Codex 接入说明](plugins/wingmate-backends/README.md)

项目以个人使用为主，不接受 Pull Request（PR）。欢迎按适用许可证 fork 后自行二次开发。

## 使用

发行目标为 Windows、macOS 和 Linux。请选择与系统和处理器架构对应的构建，并安装相同架构的 Node.js >=24.9.0。桌面构建在 Windows 需要 WebView2，macOS 使用系统 WebKit，Linux 需要 WebKitGTK 4.1 及对应桌面运行库。无 GUI 构建不链接 Tauri 或 WebView。Zstd 解压库已内嵌。各平台系统依赖见 [Tauri 前提条件](https://v2.tauri.app/start/prerequisites/)。

Word、PowerPoint 及部分扩展格式使用随包提供的 [File Viewer 侧栏预览](plugins/wingmate-file-viewer/README.md)，发行包已移除 LibreOffice 引擎。能力范围与相对原版的取舍见[运行时维护指南](docs/runtime-maintenance.md#去重与精简取舍)。

侧栏底部可在 DSH 和 Codex 间切换工作台，切换后原后端的任务继续运行。打开侧栏「插件 → 内置插件配置」可查看 Wingmate 的三张插件卡片，并在「Wingmate 执行后端」检查 Codex 连接。安装方式和能力范围见[执行后端插件](plugins/wingmate-backends/README.md)。

成品可单独复制运行，Windows 为 `target/release/wingmate.exe`，macOS 和 Linux 为 `target/release/wingmate`。打开桌面，在左下角账号菜单进入「设置 → 后台服务」，可查看、停止或重启 DSH，也可为当前用户启用 `wingmate` 和 `dsh` 终端命令。停止或重启 DSH 会中断运行任务。后台停止后管理服务继续存在，可在 Wingmate 首页或 CLI 再次启动。`service-stop` 仅在业务已停止后退出管理服务，`service-restart` 显式切换管理服务组件。

在 Windows 标题栏点「连接」可粘贴另一台主机的链接，或复制供本机浏览器使用的配对链接。开启局域网访问后，还可分享局域网配对链接和二维码。详细分享设置位于「插件 → 内置插件配置 → Wingmate 后台服务」。见[桌面连接面板](apps/desktop/README.md)和 [Web 入口](crates/wingmate-service/README.md#web-入口)。

在终端运行 `wingmate --help` 或 `wingmate -h` 可查看后台管理命令。可执行文件尚未加入 PATH 时，用完整或相对路径调用，例如：

```sh
./target/release/wingmate status
```

```powershell
& ./target/release/wingmate.exe status | Out-Host
```

服务器可执行 `wingmate start` 启动业务，或执行 `wingmate service-start` 准备运行内容并只启动管理服务。`wingmate commands on` 在业务运行时注册当前用户命令，`wingmate commands off` 可在后台停止后撤销注册。

桌面和管理服务都从受管副本运行，外层可执行文件可在没有引导命令执行时替换，已有任务继续。打开新版先连接现有服务和业务，选择「重启并更新」或执行 `wingmate restart` 可切换待更新组件。管理服务也可在业务停止后通过 `service-restart` 单独切换。不提供旧后台协议迁移，请在使用此架构前显式停止旧版后台。数据位置见[服务模块](crates/wingmate-service/README.md)，组件规则见[运行时维护指南](docs/runtime-maintenance.md#释放与复用)。

## 开发入口

在目标平台和架构上使用同架构的 Node/npm、Rust 和 pnpm；构建桌面时另需平台 Tauri 构建工具。本地发行和验收规则见[运行时维护指南](docs/runtime-maintenance.md#打包范围与所有权)。

```powershell
pnpm install --frozen-lockfile
pnpm dev
pnpm test
```

日常开发和运行验证用 `pnpm dev`，由它准备并复用开发运行时。开发目录和停机步骤见[本地开发与构建](docs/runtime-debugging.md#本地开发与构建)。

遇到明确的发行版问题或需要验收发行产物时，使用以下命令。无 GUI 发行使用 `pnpm build --headless`。

```powershell
pnpm build
pnpm test:smoke
```

验证范围见[验证约定](scripts/README.md)。测试专用目录按[收尾步骤](docs/runtime-debugging.md#收尾)清理。

## 文档与模块

- [AGENTS.md](AGENTS.md)：按任务选择文档及维护文档的约定。
- [项目约定](docs/product.md)：产品边界、当前选择与未来方向。
- [临时架构与实施方案](ARCHITECTURE_PLAN.md)：已确定的多内核、远程和多端设计，模块归属及待实施任务；完成后迁移约定并移除。
- [运行时维护指南](docs/runtime-maintenance.md)：DSH 获取、升级、精简取舍、适配和插件集成。

| 模块 | 责任 |
| --- | --- |
| [src-tauri](src-tauri/README.md) | 桌面宿主与用户操作入口 |
| [wingmate-launcher](crates/wingmate-launcher/README.md) | 无 GUI 发行入口、资源释放、版本协调与命令注册 |
| [wingmate-service](crates/wingmate-service/README.md) | 独立管理服务与业务生命周期 |
| [apps/desktop](apps/desktop/README.md) | 自有桌面界面 |
| [runtime](runtime/README.md) | 发行运行内容 |
| [plugins](plugins/README.md) | 自用插件、DSH 开发参考与本地验证入口 |
| [upstream](upstream/README.md) | 外部源码与参考项目 |

[apps](apps/README.md)、[crates](crates/README.md)、[packages](packages/README.md) 分别承载前端应用、Rust 库和实际需要共享的前端/JavaScript 包。

## 上游项目与许可

Wingmate 是独立维护的社区发行版。Wingmate 自有代码默认采用 [MIT 许可证](LICENSE)。移植代码、按其他许可证发布的组合产物和随包依赖适用各自的许可条款。

- **DeepSeek Harness** 提供 Agent、Web 工作台和插件生态，所用版本采用 MIT 许可。[项目](https://github.com/deepseek-ai/deepseek-harness) · [版本许可](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.1/LICENSE)
- **dsh-Go** 是 Android 本地运行时的移植来源。采用的载荷构建脚本及本机引擎代码涉及 AGPLv3。[项目](https://github.com/ccch1mneyyy/dsh-Go) · [本地运行载荷](runtime/android/README.md) · [上游许可](https://github.com/ccch1mneyyy/dsh-Go/blob/288234ae52fdfcfc2b0a0388763c371d0b09ea7d/LICENSE)
- **deepseek-harness-desktop** 是 Android 连接界面、工作台通知脚本和公网隧道下载逻辑的移植来源。上游许可由 MIT 文本和限制商业二次开发的附加条款组成。[项目](https://github.com/dsh-tauri/deepseek-harness-desktop) · [Android 采用记录](apps/mobile/README.md#上游来源) · [隧道采用记录](crates/wingmate-service/README.md#公网隧道)
- **ds-harness-remote** 是外部 Agent 工作区与会话投影的参考，相关代码保留 MIT 声明。[项目](https://github.com/liguobao/ds-harness-remote) · [执行后端插件](plugins/wingmate-backends/README.md)
