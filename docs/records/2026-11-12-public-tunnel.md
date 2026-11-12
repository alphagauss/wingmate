# 2026-11-12 公网隧道接入

Wingmate 在独立管理服务上提供临时和具名 Cloudflare 隧道。使用与配置见[服务模块](../../crates/wingmate-service/README.md#公网隧道)和[桌面连接面板](../../apps/desktop/README.md)。

## Windows x64 验证

- 从官方发布元数据取得 Windows x64 连接器的摘要，下载实际二进制，校验 SHA-256 和版本输出后发布。直接下载失败时验证了镜像回退。将下载时限从两分钟延长后完成了慢速下载。
- 服务集成测试使用模拟连接器覆盖两种启动参数、回环回源、公开 Host 和 HTTPS Origin 校验、一次性配对、Secure Cookie、Token 隐藏及隧道关闭。无业务后台时，配对后可打开恢复页。
- 在实际 Tauri WebView 中检查三个标签页、具名详情页跳转和保存、模式切换、清除上一个地址的二维码、两种模式的配对二维码和关闭。该用例使用模拟连接器。
- 在实际 Tauri WebView 中开启临时隧道的首次下载，确认真实下载进度显示百分比。关闭后进度条消失，连接器没有启动，下载临时目录已清理。

Cloudflare 公网链路留待单独验证。真实域名下的资源加载、聊天流式输出、文件上传下载、重连和 `/plugins/events` 尚无 Wingmate 实测结果。macOS 和 Linux 未运行。
