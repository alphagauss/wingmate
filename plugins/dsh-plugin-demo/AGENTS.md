# 插件示例开发约定

本项目是可独立运行的 DSH 文本统计插件。直接把本目录设为工作区即可开发，不依赖 Wingmate 仓库、上级目录的依赖或 Git。暂由 Wingmate 主仓库跟踪，本目录不另建 Git。

## 代码边界

UI 使用 TSX，算法与业务使用 TS。ui 提供共用界面；core 只做纯计算；server 提供共用后台业务；dsh 注册官方插件接口；web 提供独立网页。两种入口不得复制业务规则。字符按 Unicode 码点统计，词组按空白分隔，空文本为零行，不保存用户输入。

## 命令与验证

在本目录执行 pnpm install --frozen-lockfile，prepare 会构建产物。pnpm dev 启动带 React 热更新的独立网页；pnpm build 执行类型检查并生成正式产物；pnpm start 只运行成品；pnpm test 检查算法和真实 HTTP 行为。使用 Node 24。依赖和锁文件在本目录独立维护。

修改后执行 pnpm typecheck、pnpm test、pnpm build。检查打包文件包含声明的入口、补丁及展示资源。开发工具放在 devDependencies，普通运行只使用编译产物。不在加载插件时编译或监听源码。

## DSH 接入

接口以目标 DSH 的运行时检查、包文档与实现为准。创造模式先加载官方 `dsh-doc` 技能并查阅其中的 `references/plugin-development.md`。安装和启停通过 plugin_manager，外部终端使用指向目标环境的 dsh plugin 命令。不要手写用户 Profile、修改已安装包或默认用户目录。

源码安装与更新先重新构建并打包，再安装成品，避免链接工作副本影响正常使用。Git 源码安装通过标准 prepare 构建。编译失败不替换正在使用的版本。以安装返回的 application、warnings 和实际功能判断成功，restart-required 不等于已生效。

侧栏和页面通过官方 slot 注册，使用宿主 React 和主题。定时器、事件、路由及其他注册随插件卸载清理。详情页提供使用说明，开发规则只维护在本文件。

## 插件 UI 与动效

- 先按目标 DSH 版本找最接近的官方界面。设置项对照设置页，插件详情对照内置插件详情；从对应发布 tag 的 [UI/UX 指引](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.5/.agents/skills/dsh-client-ui-ux/SKILL.md)和[组件目录](https://github.com/deepseek-ai/deepseek-harness/blob/dsh-v0.2.2-rc.5/packages/client/ui-primitives/README.zh.md#component-catalog)确认可用接口。链接以本模板当前版本为例，升级时切换到目标 tag。
- DSH 入口使用宿主 React 和 `@deepseek-ai/dsh-client-ui-primitives` 的控件。例如弹窗用 `Modal`，由它处理关闭、焦点和浮层行为。插件布局使用宿主主题的 `--dsw-*` 变量，样式限定在自己的界面内。另有独立网页入口时，在该入口提供需要的控件和主题回退。
- 动效跟随同类交互。列表需要展示用户操作引起的增删或重排时，优先用 `AnimatedRows`；首次加载和刷新时直接呈现数据。自定义过渡应能说明它表达的状态变化，并在 `prefers-reduced-motion: reduce` 下保留清晰的静态反馈。
- 对照实际 DSH 页面检查本次改动涉及的深浅主题、窄窗口、键盘焦点及减少动态效果。涉及动画时分别观察首次显示和用户操作后的变化。

## Wingmate 开发调试

先从 Wingmate 仓库启动 pnpm dev。再在本项目执行 pnpm dev:dsh；项目位于仓库外时传入 --runtime <Wingmate 仓库>/work/dev/wingmate/runtime.json。脚本通过开发版 CLI 获取专用本机会话，自动编译 .dev/dsh 并调用官方安装流程更新开发链接；它不改正式 dist。退出时关闭源码监听并撤销专用会话，最后编译的开发链接保留在开发环境中。正常使用环境通过“编译并更新”安装成品，不运行此命令。

作为新插件模板使用时，修改 package.json 的 name、cordis.patch.yml 的名称及 id、src/types.ts 的 pluginName，并同步标题、图标和 README。客户端 factory 自动读取包名。开发调试和正式构建共用构建脚本，编译结果分别存放。

修改构建脚本后重启开发命令；修改依赖后先完成 pnpm install。源码与语言资源保存后自动更新，首次从成品切换到开发链接若返回 restart-required，按提示重启开发后台后继续。
