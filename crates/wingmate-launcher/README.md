# Wingmate 发行入口

不依赖 Tauri、WebView 或桌面会话，负责 CLI、发行组件、版本协调、资源释放、服务引导和本机命令注册。桌面只通过此入口请求准备或切换运行内容。

运行资源、服务可执行文件和可选桌面组件并列携带；资源不内嵌到管理服务。服务和桌面从按内容指纹释放的副本运行，避免占用发行入口和构建输出。开发模式也使用释放副本。

引导操作由 `bootstrap.lock` 串行化，业务组件安装和回收与服务启停共用 `operation.lock`。`wingmate restart` 停止业务并切换发行入口携带的变更组件。服务指纹变化时同时更换管理服务。组件发布与回收见[运行时维护指南](../../docs/runtime-maintenance.md#释放与复用)。

`stop` 停止业务并保留管理服务；`service-stop` 仅在业务已停止时退出服务。服务意外退出后，`start` 可恢复管理连接。当前不提供旧发现协议的迁移路径。

局域网分享可用 `wingmate share-*` 管理，公网隧道可用 `wingmate tunnel-*` 管理。启用和配对见[服务模块](../wingmate-service/README.md#web-入口)。

命令与登录启动注册属于本模块，指向外层发行入口，不指向指纹目录中的服务或桌面副本。桌面设置只调用注册接口。运行组件和数据规则见[运行时维护指南](../../docs/runtime-maintenance.md)与[服务模块](../wingmate-service/README.md)。
