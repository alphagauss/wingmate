# Wingmate

Wingmate 是面向个人使用的 DeepSeek Harness 发行版，提供运行时分发和后台管理，复用官方 Web 和插件生态。

当前首个入口是桌面端。单个可执行文件承载桌面与常驻后台，关闭窗口后任务继续运行。后续方向见[项目约定](docs/product.md#未来方向尚未交付)。

## 使用

发行目标为 Windows、macOS 和 Linux。请选择与系统和处理器架构对应的构建，并安装相同架构的 Node.js >=24.9.0。Windows 需要 WebView2，macOS 使用系统 WebKit，Linux 需要 WebKitGTK 4.1 及对应桌面运行库。Zstd 解压库已内嵌。各平台系统依赖见 [Tauri 前提条件](https://v2.tauri.app/start/prerequisites/)。

Word、PowerPoint 及部分扩展格式使用随包提供的 [File Viewer 侧栏预览](plugins/wingmate-file-viewer/README.md)，发行包已移除 LibreOffice 引擎。能力范围与相对原版的取舍见[运行时维护指南](docs/runtime-maintenance.md#去重与精简取舍)。

成品可单独复制运行，Windows 为 `target/release/wingmate.exe`，macOS 和 Linux 为 `target/release/wingmate`。打开桌面，在左下角账号菜单进入「设置 → 后台服务」，可查看、停止或重启后台，也可为当前用户启用 `dsh` 终端命令。停止或重启会中断运行任务。后台停止后可在 Wingmate 首页再次启动。

终端也可以用 `wingmate status`、`wingmate start`、`wingmate stop` 和 `wingmate restart` 管理后台。可执行文件尚未加入 PATH 时，用完整或相对路径调用，例如：

```sh
./target/release/wingmate status
```

```powershell
& ./target/release/wingmate.exe status | Out-Host
```

关闭所有桌面窗口后即可替换可执行文件，运行中的 DSH 任务会继续。新版携带不同的运行包且 DSH 版本相同时，首页会询问是继续连接还是重启切换。若要切换 DSH 版本，请先用原版本显式停止后台。数据位置见 [服务模块](crates/wingmate-service/README.md)，运行包释放及切换见 [运行时维护指南](docs/runtime-maintenance.md#释放与复用)。

## 开发入口

在目标平台和架构上使用同架构的 Node/npm、Rust、pnpm 及平台 Tauri 构建工具。本地发行和验收规则见[运行时维护指南](docs/runtime-maintenance.md#打包范围与所有权)。

```powershell
pnpm install --frozen-lockfile
pnpm dev
pnpm build
```

`pnpm dev` 复用 DSH 依赖并准备插件运行目录，再启动桌面，开发后台和正式后台可以同时运行。开发窗口关闭后，运行 `pnpm dev:stop` 可以停止本工作副本的开发后台。依赖复用与自动清理见 [运行时维护指南](docs/runtime-maintenance.md#打包范围与所有权)，数据目录见 [服务模块](crates/wingmate-service/README.md#数据与管理边界)。

验证入口：

```powershell
pnpm test
pnpm build
pnpm test:smoke
```

命令定义以 [package.json](package.json) 为准；选择验证范围和解释结果时阅读 [验证约定](scripts/README.md)。

## 文档与模块

- [AGENTS.md](AGENTS.md)：按任务选择文档及维护文档的约定。
- [项目约定](docs/product.md)：产品边界、当前选择与未来方向。
- [运行时维护指南](docs/runtime-maintenance.md)：DSH 获取、升级、精简取舍、适配和插件集成。

| 模块 | 责任 |
| --- | --- |
| [src-tauri](src-tauri/README.md) | 桌面宿主与用户操作入口 |
| [wingmate-service](crates/wingmate-service/README.md) | 本机后台所有权与生命周期 |
| [apps/desktop](apps/desktop/README.md) | 自有桌面界面 |
| [runtime](runtime/README.md) | 发行运行内容 |
| [plugins](plugins/README.md) | 自用插件、DSH 开发参考与本地验证入口 |
| [upstream](upstream/README.md) | 外部源码与参考项目 |

[apps](apps/README.md)、[crates](crates/README.md)、[packages](packages/README.md) 分别承载前端应用、Rust 库和实际需要共享的前端/JavaScript 包。

Wingmate 是自用社区封装，与 [DeepSeek Harness 官方项目](https://github.com/deepseek-ai/deepseek-harness) 无隶属关系。发行内容保留各组件要求的许可证及声明。
