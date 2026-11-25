# Wingmate 插件开发

电脑端随包提供。在「插件 → 内置插件配置 → Wingmate 插件开发」关联源码目录，然后点击「编译并更新」。移除关联不卸载插件；启停和卸载仍使用官方插件页面。离开开发页面会取消尚未完成的构建请求。

为独立插件提供源码关联、正式构建和安装更新。编译逻辑由插件的 build 命令定义，安装、启停和加载结果沿用 DSH 官方插件管理器；不另存一份安装状态。

源码关联按当前 DSH Profile 保存到 wingmate-devtools/sources.json。构建产物打包后按内容摘要保存在同目录的 packages 中，供 Profile 后续安装和恢复使用。清除关联不卸载插件，也不删除源码或用户数据。随包插件由 Wingmate 发行更新，此入口只管理独立插件。

每次更新先准备依赖、执行 build，再用 npm pack --ignore-scripts 生成成品。锁文件存在时使用冻结安装。prepare 可能在准备依赖时执行；显式 build 保证没有 prepare 的项目也能生成最新产物。编译失败不调用安装。包安装和激活失败、重启要求按 DSH 的结果报告，不承诺激活失败自动恢复旧代码。多个请求不能同时构建同一个插件，取消或卸载会结束构建子进程。

正常使用不监听源码，不运行编译器或开发服务器。开发监听只由显式开发命令启动。编译工具从 Wingmate 随包工具或开发机工具中定位，插件成品不携带编译依赖。

运行 pnpm --filter dsh-wingmate-devtools build 和 test 验证构建、目录关联、取消、失败及官方安装结果的保留。

创造模式和其他 DSH Agent 可调用 wingmate_plugin 工具，list、register、build、forget 与界面共用后台操作。工具按当前会话的沙箱模式要求完整权限或本次批准，拒绝与取消不执行操作。Web 入口使用 DSH Connection 的 owner 专用认证路由，不暴露无认证的构建接口。

## 开发调试与自举

随包注册 wingmate-plugin-development 技能，补充官方创造模式的开发指引。完整 demo 模板由主仓库的 plugins/dsh-plugin-demo 生成，随本插件保存在 dist/template；源码不重复维护，开发运行组件的缓存也计入模板输入。模板可复制到任意工作区，根目录 README 用于默认插件详情，AGENTS.md 用于开发。复制后将 gitignore 重命名为 .gitignore；npm 归档会过滤原名。

demo 的 pnpm dev:dsh 只连接 Wingmate 开发后台，通过 CLI 创建并在退出时撤销专用本机会话。编译结果写入 .dev/dsh，preview 操作通过官方管理器切换或重新加载开发链接。首次切换可能替换开发环境中的已安装副本，后续更新只重载该插件；不会为普通运行配置源码监听。退出后保留最后的开发产物和链接。构建脚本或依赖变化需重新准备开发命令。

源码监听只由开发命令持有。重新编译 Wingmate 仅用于维护随包开发工具本身，独立插件的开发与更新不经过 Wingmate 构建。DSH 的实际加载结果仍决定是否需要重启。
