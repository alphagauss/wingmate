# Wingmate 后台服务

Wingmate 自行维护的 DSH Web 插件。它在官方设置页的 `settings.section` 扩展点提供「后台服务」，把已确认的停止和重启请求交给 Wingmate 桌面。后台离线后的启动与恢复由 [Wingmate 首页](../../apps/desktop/README.md) 承接。

设置页显示当前后台的本机 Web 地址。「复制链接」会复制完整的 DSH 登录链接，可粘贴到同一台电脑上的浏览器中使用。页面只显示地址，登录 token 随链接复制。

客户端采用 DSH 的 lazy factory 格式，复用其 React 和控件，不需要额外的运行依赖。DSH 兼容范围以本包的 `engines.dsh` 为准，升级时重点检查客户端清单、设置扩展点、控件和启动补丁。插件属于 Wingmate 源码，复用 DSH 官方扩展点，无上游补丁。

插件随开发安装树和发行归档提供，Wingmate 启动 `web` 时默认通过[运行包启动补丁](../cordis.patch.yml)加载。插件包内的 `cordis.patch.yml` 供 DSH Profile 单独安装此包时使用。用户的 Profile、凭据和会话仍由 DSH 管理。普通浏览器、TUI 和移动端不提供此桌面管理入口。从启动补丁移除此插件的条目后设置项消失，CLI 和首页恢复能力继续可用。

验收覆盖设置页显示 DSH 版本、取消与确认操作、停止回首页、首页重新启动、重启自动连接、普通浏览器不显示管理项，以及同版本旧后台可继续运行并在确认重启后启用插件。发行校验须使用复制出来的单文件 EXE，详见 [验证约定](../../scripts/README.md)。
