# Wingmate 后台服务

Wingmate 自行维护的 DSH Web 插件。它在官方设置页的 `settings.section` 扩展点提供「后台服务」。本机桌面通过桥接管理，已配对的浏览器和远程桌面通过同源网关管理当前主机。后台离线后的启动与恢复由[共享启动页](../../apps/launcher/README.md)承接。

本插件通过 DSH 的 `plugins.item` 在「内置插件配置」注册三张 Wingmate 卡片。各详情页显示加载状态、实际注册的预览格式或后端连接诊断。本机桌面的「Wingmate 后台服务」卡片可开关局域网访问及撤销已配对设备，开启后显示实际监听地址。配对链接和二维码在[桌面连接浮窗](../../apps/desktop/README.md)生成。普通插件的安装、启停和卸载由 DSH 插件页管理。升级 DSH 时核对卡片扩展点和设置页导航。

设置页的「DSH 本机地址」只在本机桌面或从本机 Web 入口连接时显示。「Wingmate 连接地址」可复制，本机桌面显示本机网关地址，其他客户端显示当前连接地址。复制的地址可收藏，首次访问按[配对规则](../../crates/wingmate-service/README.md#web-入口)授权。

「停止 DSH」和「重启 DSH」由当前主机的管理服务执行。主机本机桌面发现待切换组件时，提供「重启并更新」。「DSH 数据目录」显示当前后台实际使用的目录，可用系统文件管理器打开。

登录启动和启用终端命令的平台约定见[桌面程序](../../src-tauri/README.md)。此前设置完善的验证见[2026-11-10 记录](../../docs/records/2026-11-10-backend-settings.md)。

客户端采用 DSH 的 lazy factory 格式，复用其 React 和控件，不需要额外的运行依赖。DSH 兼容范围以本包的 `engines.dsh` 为准，升级时重点检查客户端清单、设置扩展点、控件和启动补丁。插件属于 Wingmate 源码，复用 DSH 官方扩展点，无上游补丁。

插件仅在收到 Windows 桌面主 WebView 为本机工作台注入的桥接后，才通过 `shell.overlay` 随工作台布局挂载和卸载[桌面标题栏](../../src-tauri/README.md#windows-标题栏)。

插件随开发安装树和发行归档提供，Wingmate 启动 `web` 时默认通过[运行包启动补丁](../cordis.patch.yml)加载。插件包内的 `cordis.patch.yml` 供 DSH Profile 单独安装此包时使用。用户的 Profile、凭据和会话仍由 DSH 管理。浏览器从 Wingmate 网关进入时可管理当前主机的启停，直接打开 DSH 的浏览器只提供 DSH 工作台。从启动补丁移除此插件的条目后设置项消失，CLI 和首页恢复能力继续可用。

验证范围包括设置页的本机管理、远程管理目标、分享卡片及业务停机后的恢复。发行校验使用复制出来的单文件可执行程序，详见[验证约定](../../scripts/README.md)。
