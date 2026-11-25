# 随包 DSH 插件

本目录承载 Wingmate 自用插件源码。[文件预览](wingmate-file-viewer/README.md)由电脑端和 Android 本地版共用。电脑端另带 [Wingmate 后台服务](wingmate-web/README.md)和[执行后端](wingmate-backends/README.md)，Android 本地版另带 [Android 本机 Agent 环境](wingmate-android-environment/README.md)。发行配置见[运行时维护指南](../docs/runtime-maintenance.md#社区和自用插件集成)。

在 DSH 侧栏打开「插件」，从「内置插件配置」下的 Wingmate 卡片进入各自详情。预装插件随 Wingmate 发行包更新。

插件遵循所选 DSH 的插件与 Profile 机制，避免在 Wingmate 中另建一套业务插件协议。普通前端共享组件归 [packages](../packages/README.md)。

开发参考和验证流程见[独立插件入口](../plugins/README.md)。
