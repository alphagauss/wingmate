# 桌面程序

负责桌面客户端与原生界面。资源准备、版本协调和命令注册交由[无 GUI 发行入口](../crates/wingmate-launcher/README.md)，后台所有权交由 [服务模块](../crates/wingmate-service/README.md)，窗口只消费服务能力；跨模块使用预期见 [项目约定](../docs/product.md)。

## 用户操作与权限边界

- 用户能区分关闭窗口、停止后台和重启后台。后两者会影响正在运行的任务，不能把关闭窗口隐式转换为停止。
- 启停或旧运行目录回收期间，窗口可以立即关闭，桌面进程会等这些操作结束再退出，避免重启或回收停在中间状态。
- 连接暂时中断不应立即刷新或销毁用户正在编辑的页面，也不能据此直接重启后台。
- 官方 Web 页面在桌面中显示，不因此获得任意 Tauri 本机能力。新增本机能力时需明确调用方和用户操作含义。
- 当前 DSH 工作台直接请求新窗口打开普通 HTTPS 链接时，Wingmate 显示目标网站。用户确认后交给系统默认浏览器，同一时间只处理一个确认。DeepSeek 登录授权地址直接交给浏览器。登录使用的临时隐藏窗口只转交该授权地址，授权回调和凭据由 DSH 后台处理。
- 外链确认框从属于发起请求的桌面窗口。Windows 相对父窗口居中，Linux 通过 GTK 请求相对父窗口居中，macOS 使用系统附属面板。
- 「设置 → 后台服务」从已验证的工作台请求停止、重启或更新，并绑定后台实例。普通重启和网关调用同一个 DSH 重启操作。检测到待切换组件时，本机的「重启并更新」通过发行入口执行。首页的启动、重试和组件切换使用独立的本地页面权限。
- 打开桌面时，管理服务或 DSH 运行包的指纹变化会在首页列出待更新组件。用户可继续使用或重启并更新。确认更新绑定当时的管理服务与业务实例；更新流程由[发行入口](../crates/wingmate-launcher/README.md)执行。
- 远程工作台使用独立 WebView，连接浮层和标题栏由本地应用提供。远程页面经当前主机的网关管理其后台。本机命令注册、系统目录与窗口权限只授予本地应用和经验证的本机工作台。
- Windows、macOS 及支持 XDG Autostart 的 Linux 桌面可在发行版的「设置 → 后台服务」为当前用户注册登录启动，默认关闭。登录时调用 Wingmate 的 `start`，使用能找到 Node 的 PATH。Windows 和 Linux 沿用登录会话的 PATH，macOS 登录项使用注册时 Wingmate 进程的 PATH。请将可执行文件放在固定位置，移动后可从设置重新注册。
- Windows、macOS 和 Linux 的发行版可在「设置 → 后台服务」启用终端命令，默认关闭。打开新终端后，`wingmate --help` 或 `wingmate -h` 会列出后台管理命令。启动器指向注册时的 Wingmate 程序和后台管理目录，`dsh` 则使用该后台的 DSH 运行时和数据目录，并按上游规则启动独立的 Profile。移动 Wingmate 或切换运行包后，可在设置中重新注册。
- Windows 将启动器目录放在用户 PATH 的最前面，系统 PATH 和 shell 别名仍可优先。macOS 和 Linux 通过 Bash 的交互和登录配置、Zsh 的交互配置、Fish 的用户配置，将 `~/.wingmate/bin` 放到 PATH 前面。使用其他 shell 时可在其配置中加入该目录。关闭开关会删除 Wingmate 启动器及其管理的 PATH 配置，已安装的其他 `dsh` 可继续使用。
- 后台和管理命令通过独立发行入口在没有窗口或 GUI 依赖时工作，界面层不复制一套后台生命周期策略。

自有界面的范围见 [桌面前端](../apps/desktop/README.md)。

开发窗口的原生标题和自绘标题栏均显示「Wingmate · 开发版」。WebView 数据保存在开发后台管理目录下，避免与发行窗口共用本机 Web 登录状态。后台与 DSH 数据的隔离见 [服务模块](../crates/wingmate-service/README.md#数据与管理边界)。

## 窗口启动与恢复

同一管理目录下，第一个打开的桌面窗口使用固定 ID `main`。关闭时保存窗口位置、尺寸和最大化状态，下次打开主窗口时恢复。主窗口打开期间再次启动会得到独立的临时窗口，按默认布局打开。主窗口关闭后，下次启动可以接任，已打开的临时窗口继续使用各自的 ID。

默认尺寸采用 [DSH 桌面配置](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.1/apps/desktop/src/main.ts#L259-L265)，默认 1280 × 820 DIP，最小 520 × 600 DIP。启动时按显示器工作区校正位置和大小。原显示器不可用时改用主显示器的默认定位，工作区太小时允许缩小到默认最小尺寸以下。

窗口走默认定位时优先居中。如果中心位置与同一管理目录下已打开窗口的左上角太接近，按 32 DIP 向右下层叠，到工作区边缘后检查其余候选位置。候选位置用尽时居中。窗口坐标只在运行期间登记，最小化的窗口不占位置，临时窗口的布局不用于恢复。异常退出留下的登记在下次启动时清理。

## Windows 标题栏

首页和工作台共用 40 DIP 的自绘窗口控件。工作台沿用 [DSH 桌面布局](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.1/packages/client/ui-layout/README.zh.md#窗口-chrome-座) 的顶栏避让和主题颜色。首页初次打开跟随系统，返回首页时沿用本窗口上次收到的 DSH 明暗主题选择。三个按钮的绘制参考 [Electron Windows 窗口按钮](https://github.com/electron/electron/blob/v44.7.0/shell/browser/ui/views/win_caption_button.cc)，窗口操作由 Tauri 执行。标题栏拖动和双击使用 WebView2 的拖动区域。

Windows 桌面窗口以无系统标题栏的形式打开，导航时保留当前边框。新页面未在 5 秒内挂载自绘控件时恢复系统标题栏，旧后台和页面加载失败时仍可操作窗口。窗口操作权限仅授予桌面窗口的首页和已验证的工作台来源。

## 品牌与桌面图标

[品牌母版](icons/logo.svg)使用透明底蓝鲸。[桌面图标](icons/icon.svg)沿用同一鲸形，蓝色圆角底板为不同壁纸上的白鲸提供稳定对比，底板外保持透明。

更新轮廓或对齐时同步两份 SVG 和 Android 首页、矢量前景。用 `pnpm tauri icon src-tauri/icons/icon.svg --output <临时目录>` 生成图标，取 `icon.ico` 供 Windows EXE 使用，取 `icon.png` 供 Tauri 在其他桌面平台回退使用。Android 旧版系统图标取生成目录中各密度下的 `ic_launcher.png`。
