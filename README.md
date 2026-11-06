# Wingmate

Wingmate 是面向个人使用的 DeepSeek Harness 发行版，提供运行时分发和后台管理，复用官方 Web 和插件生态。

当前首个入口是 Windows 桌面端。单个可执行文件承载桌面与常驻后台，关闭窗口后任务继续运行。后续计划支持 TUI、移动端和远程连接，详见 [项目约定](docs/product.md#未来方向尚未交付)。

## 使用

当前优先支持 Windows x64。运行需要系统 Node.js >=24.9.0 和 WebView2；Node 暂不随程序分发。Zstd 解压库已内嵌，无需额外安装。

成品为 `target/release/wingmate.exe`，可以单独复制运行。双击打开桌面，“后台”菜单提供连接、停止和重启。停止或重启会中断运行任务。

也可在 PowerShell 中管理后台：

```powershell
& ./target/release/wingmate.exe status | Out-Host
& ./target/release/wingmate.exe start | Out-Host
& ./target/release/wingmate.exe stop | Out-Host
& ./target/release/wingmate.exe restart | Out-Host
```

关闭所有桌面窗口后即可替换 Windows EXE，运行中的 DSH 任务会继续。若要切换 DSH 版本，请先用原版本显式停止后台。数据位置见 [服务模块](crates/wingmate-service/README.md)，版本目录复用及重新释放方式见 [运行时维护指南](docs/runtime-maintenance.md#释放与复用)。

## 开发入口

构建需要 Node/npm、pnpm、Rust 及平台 Tauri 构建工具。当前在目标平台本机构建，尚未验证跨平台发行。

```powershell
pnpm install --frozen-lockfile
pnpm dev
pnpm build
```

验证入口：

```powershell
pnpm runtime:prepare
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
| [plugins](plugins/README.md) | 自用插件及集成边界 |
| [upstream](upstream/README.md) | 外部源码与参考项目 |

[apps](apps/README.md)、[crates](crates/README.md)、[packages](packages/README.md) 分别承载前端应用、Rust 库和实际需要共享的前端/JavaScript 包。

Wingmate 是自用社区封装，与 [DeepSeek Harness 官方项目](https://github.com/deepseek-ai/deepseek-harness) 无隶属关系。发行内容保留各组件要求的许可证及声明。
