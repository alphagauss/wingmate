# 桌面程序

负责桌面宿主与用户管理入口。后台所有权交由 [服务模块](../crates/wingmate-service/README.md)，窗口只消费服务能力；跨模块使用预期见 [项目约定](../docs/product.md)。

## 用户操作与权限边界

- 用户能区分关闭窗口、停止后台和重启后台。后两者会影响正在运行的任务，不能把关闭窗口隐式转换为停止。
- 后台启停操作已开始时，关闭窗口会让桌面进程在操作结束后退出，避免重启停在旧后台已退出的中间状态。
- 连接暂时中断不应立即刷新或销毁用户正在编辑的页面，也不能据此直接重启后台。
- 官方 Web 页面在桌面中显示，不因此获得任意 Tauri 本机能力。新增本机能力时需明确调用方和用户操作含义。
- 当前 DSH 工作台直接请求新窗口打开普通 HTTPS 链接时，Wingmate 显示目标网站。用户确认后交给系统默认浏览器，同一时间只处理一个确认。DeepSeek 登录授权地址直接交给浏览器。登录使用的临时隐藏窗口只转交该授权地址，授权回调和凭据由 DSH 后台处理。
- 外链确认框从属于主窗口。Windows 相对主窗口居中，Linux 通过 GTK 请求相对父窗口居中，macOS 使用系统附属面板。
- 「设置 → 后台服务」通过桌面窗口调用启停。停止和重启只接受当前已验证工作台发出的请求，并绑定后台实例。首页的启动、重试和运行包切换入口使用独立的本地页面权限。
- 后台和管理命令可以在没有窗口的情况下工作，界面层不复制一套后台生命周期策略。

自有界面的范围见 [桌面前端](../apps/desktop/README.md)。

开发窗口的原生标题和自绘标题栏均显示「Wingmate · 开发版」。WebView 数据保存在开发后台管理目录下，避免与发行窗口共用本机 Web 登录状态。后台与 DSH 数据的隔离见 [服务模块](../crates/wingmate-service/README.md#数据与管理边界)。

## Windows 标题栏

首页和工作台共用 40 DIP 的自绘窗口控件。工作台沿用 [DSH 桌面布局](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.1/packages/client/ui-layout/README.zh.md#窗口-chrome-座) 的顶栏避让和主题颜色。首页初次打开跟随系统，返回首页时沿用本窗口上次收到的 DSH 明暗主题选择。三个按钮的绘制参考 [Electron Windows 窗口按钮](https://github.com/electron/electron/blob/v44.7.0/shell/browser/ui/views/win_caption_button.cc)，窗口操作由 Tauri 执行。标题栏拖动和双击使用 WebView2 的拖动区域。

Windows 主窗口以无系统标题栏的形式打开，导航时保留当前边框。新页面未在 5 秒内挂载自绘控件时恢复系统标题栏，旧后台和页面加载失败时仍可操作窗口。窗口操作权限仅授予首页及主 WebView 中已验证的工作台来源。

## 品牌与桌面图标

[品牌母版](icons/logo.svg)使用透明底蓝鲸。[桌面图标](icons/icon.svg)沿用同一鲸形，蓝色圆角底板为不同壁纸上的白鲸提供稳定对比，底板外保持透明。

更新轮廓时同步两份 SVG。用 `pnpm tauri icon src-tauri/icons/icon.svg --output <临时目录>` 生成桌面图标，取 `icon.ico` 供 Windows EXE 使用，取 `icon.png` 供 Tauri 在其他桌面平台回退使用。
