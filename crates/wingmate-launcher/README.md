# Wingmate 发行入口

不依赖 Tauri、WebView 或桌面会话，负责 CLI、发行组件、版本协调、资源释放、服务引导和本机命令注册。桌面只通过此入口请求准备或切换运行内容。

运行资源、服务可执行文件和可选桌面组件并列携带；资源不内嵌到管理服务。发行版服务和桌面从管理目录内的固定路径运行，避免占用发行入口和构建输出。开发服务也使用固定路径的副本。

Windows 发行入口在链接时内嵌 [桌面图标](../../src-tauri/README.md#品牌与桌面图标)，包括 `--headless` 发行。macOS 和 Linux 继续交付裸可执行文件，文件管理器使用系统默认图标。桌面品牌图标由运行中的桌面组件提供。

桌面进程和发行入口的诊断位置见[运行时调试](../../docs/runtime-debugging.md#后台与发行产物)。

引导操作由 `bootstrap.lock` 串行化，业务组件安装和回收与服务启停共用 `operation.lock`。`wingmate restart` 停止业务并切换发行入口携带的变更组件。服务指纹变化时同时更换管理服务。组件发布与回收见[运行时维护指南](../../docs/runtime-maintenance.md#释放与复用)。

`stop` 停止业务并保留管理服务；`service-stop` 仅在业务已停止时退出服务。服务意外退出后，`start` 可恢复管理连接。当前不提供旧发现协议的迁移路径。

局域网分享可用 `wingmate share-*` 管理，公网隧道可用 `wingmate tunnel-*` 管理。启用和配对见[服务模块](../wingmate-service/README.md#web-入口)。

命令与登录启动注册属于本模块，统一指向外层发行入口。桌面设置只调用注册接口。运行组件和数据规则见[运行时维护指南](../../docs/runtime-maintenance.md)与[服务模块](../wingmate-service/README.md)。

## 终端命令注册

`wingmate install` 为当前用户注册 `wingmate` 启动器，保留注册时的发行文件路径与管理目录。注册无需运行管理服务或 DSH。移动发行文件后，用新路径重新执行 `install`。启动器运行时需要在当前会话的 PATH 中找到 Node。

`wingmate commands on` 在业务运行时同时注册 `wingmate` 和 `dsh`。`dsh` 使用该后台的运行内容和数据目录，并按上游规则启动独立 Profile。`wingmate commands off` 撤销本模块注册的启动器和 PATH 配置，也可撤销单独执行的 `install`。

Windows 将 `%LOCALAPPDATA%\Wingmate\bin` 放在用户 PATH 最前面，系统 PATH 和 shell 别名仍可优先。macOS 和 Linux 在 Bash 交互与登录配置、Zsh 的 `.zshenv` 与交互配置、Fish 用户配置中加入 `~/.wingmate/bin`。新终端会读取更新后的配置。SSH 非交互会话能否找到裸命令取决于远端 shell 和 PATH 配置，亦可直接使用发行文件的绝对路径。

`install --shell` 在注册成功后输出 PATH 更新脚本，由当前 Shell 执行后立即生效。Windows PowerShell 使用 `--shell powershell`，macOS 和 Linux 的 Bash/Zsh 使用 `--shell sh`，调用示例见[根 README](../../README.md#使用)。Fish 可运行 `./target/release/wingmate install --shell fish | source`。直接运行 `install` 会返回 JSON 安装结果，适合只注册用户 PATH 的场景。
