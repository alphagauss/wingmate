# Wingmate 发行入口

不依赖 Tauri、WebView 或桌面会话，负责 CLI、发行组件、版本协调、资源释放、服务引导和本机命令注册。桌面只通过此入口请求准备或切换运行内容。

运行资源、服务可执行文件和可选桌面组件并列携带；资源不内嵌到管理服务。发行版服务和桌面从管理目录内的固定路径运行，避免占用发行入口和构建输出。开发服务也使用固定路径的副本。

Windows 发行入口在链接时内嵌 [桌面图标](../../src-tauri/README.md#品牌与桌面图标)，包括 `--headless` 发行。macOS 桌面发行还会将完整入口封装为 `.app` 和 `.dmg`，见[打包范围与所有权](../../docs/runtime-maintenance.md#打包范围与所有权)。Linux 和无 GUI 发行继续交付裸可执行文件，文件管理器使用系统默认图标。桌面品牌图标由运行中的桌面组件提供。

桌面进程和发行入口的诊断位置见[运行时调试](../../docs/runtime-debugging.md#后台与发行产物)。

引导操作由 `bootstrap.lock` 串行化，业务组件安装和回收与服务启停共用 `operation.lock`。`wingmate restart` 停止业务并切换发行入口携带的变更组件。服务指纹变化时同时更换管理服务。组件发布与回收见[运行时维护指南](../../docs/runtime-maintenance.md#释放与复用)。

`stop` 停止业务并保留管理服务；`service-stop` 仅在业务已停止时退出服务。服务意外退出后，`start` 可恢复管理连接。当前不提供旧发现协议的迁移路径。

在 macOS 上，Wingmate 优先使用当前进程的 PATH 查找可用的 Node.js。如果检查失败，会尝试读取用户交互式登录 Shell 导出的 PATH，再检查 Node.js。这样从 Finder 启动也能使用 Shell 配置中的 nvm 等版本管理器。通过 Shell 恢复的 PATH 会传给 DSH 及其子进程。

局域网分享可用 `wingmate share-*` 管理，公网隧道可用 `wingmate tunnel-*` 管理。启用和配对见[服务模块](../wingmate-service/README.md#web-入口)。

命令与登录启动注册属于本模块，统一指向外层发行入口。桌面设置只调用注册接口。运行组件和数据规则见[运行时维护指南](../../docs/runtime-maintenance.md)与[服务模块](../wingmate-service/README.md)。

## 终端命令注册

`wingmate install` 为当前用户注册 `wingmate` 启动器，保留注册时的发行文件路径与管理目录。注册无需运行管理服务或 DSH。移动发行文件后，用新路径重新执行 `install`。

`wingmate commands on` 在业务运行时同时注册 `wingmate` 和 `dsh`。`dsh` 使用该后台的运行内容和数据目录，并按上游规则启动独立 Profile。`wingmate commands off` 撤销本模块注册的启动器和 PATH 配置，也可撤销单独执行的 `install`。

Windows 将 `%LOCALAPPDATA%\Wingmate\bin` 放在用户 PATH 最前面，系统 PATH 和 shell 别名仍可优先。macOS 和 Linux 在 Bash 交互与登录配置、Zsh 的 `.zshenv` 与交互配置、Fish 用户配置中加入 `~/.wingmate/bin`。新终端会读取更新后的配置。SSH 非交互会话能否找到裸命令取决于远端 shell 和 PATH 配置，亦可直接使用发行文件的绝对路径。

`install --shell` 在注册成功后输出 PATH 更新脚本，由当前 Shell 执行后立即生效。Windows PowerShell 使用 `--shell powershell`，macOS 和 Linux 的 Bash/Zsh 使用 `--shell sh`，调用示例见[根 README](../../README.md#使用)。Fish 可运行 `./target/release/wingmate install --shell fish | source`。直接运行 `install` 会返回 JSON 安装结果，适合只注册用户 PATH 的场景。

`wingmate uninstall` 只撤销 `wingmate` 启动器注册。已有 `dsh` 注册时，保留其启动器和共享的 PATH 配置。发行文件、后台和用户数据会保留。Windows 旧版可用发行文件的路径执行 `uninstall`，也可用该路径执行 `install` 更新启动器后再从 PATH 调用。Windows 保留内部转发文件供下次注册复用。

## CLI 连接与诊断

- `wingmate --version` 输出发行入口的版本，运行中的管理服务指纹可通过 `wingmate service-status` 查看。
- `wingmate connect-info` 查询管理服务已监听的本机 Web 网关，返回 JSON 中的 `schema_version`、`host`、`port` 和 `url`。初始 `schema_version` 为 `1`。显式加 `--pair` 时，还会返回一次性 `pairing_url` 和以 Unix 秒为单位的 `pairing_expires_at`。每次生成链接都会替换先前尚未使用的本机配对链接。管理服务停止或网关不可用时返回非零退出码。
- `wingmate doctor` 用 JSON 报告启动时的 Node.js 检查、管理目录、管理服务、网关和业务状态。Node 或管理服务检查失败时仍返回诊断结果。如果 SSH 会话找不到 Node，可以用发行文件的绝对路径调用 `doctor`。
- `wingmate logs [host|service] [--tail <行数>]` 默认查看管理服务和业务后台各最近 100 行。指定 `host` 或 `service` 时只输出对应日志。日志文件尚未生成时会在标准错误中提示。

通过 SSH 手动连接时，在远端执行 `wingmate service-start`，然后用 `wingmate connect-info --pair` 获取本机配对链接和网关端口。SSH 转发的本地监听地址和端口应与返回的 `host` 和 `port` 一致，浏览器可直接打开原配对链接。现有网关会校验请求的 Host 和 Origin。见[本机 Web 入口](../wingmate-service/README.md#本机-web)和[后续 SSH 计划](../../ARCHITECTURE_PLAN.md#b2-网关与局域网远程启停)。
