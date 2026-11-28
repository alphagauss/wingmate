---
name: wingmate-plugin-development
description: 在 Wingmate 中创建、修改、调试和更新独立的 TS/TSX DSH 插件，支持独立网页运行及创造模式自举。
---

# Wingmate 独立插件开发

先加载 DSH 官方 `dsh-doc` 技能，查阅其中的 `references/plugin-development.md`，并按当前运行时检查结果选择接口。本技能补充项目组织、构建和交付方式；安装、启停、卸载及权限继续使用官方机制。

## 项目与模板

把插件目录设为工作区。已有项目先读其 AGENTS.md，不依赖 Wingmate 父仓库。新项目可复制本技能返回的资源目录中的 dist/template 到用户指定的空目录；它包含完整示例源码、独立锁文件及开发约定，不包含 node_modules、dist 或 Git。只有发行版 Wingmate 也能使用这些文件。

复制后把 gitignore 重命名为 .gitignore（npm 打包会排除原名）。调整 package.json 的 name、cordis.patch.yml 的插件名称与 id、src/types.ts 的 pluginName，保持一致；修改标题、图标及 README 以符合实际功能。客户端 factory 的 id 从 package.json 读取。是否创建独立 Git 按用户要求决定。

新增或修改插件界面时，先阅读技能资源目录中 `dist/template/AGENTS.md` 的「插件 UI 与动效」一节。

UI 写 TSX，算法和业务写 TS。ui、core、server 分别负责共用界面、纯计算及后台业务；dsh、web 提供两种入口，业务代码只维护一份。完整代码和具体命令以模板 AGENTS.md 为准。README 面向使用者，并作为 DSH 默认详情文档打包；不要把开发指引写进详情页。

## 开发与更新

在项目目录执行 pnpm install --frozen-lockfile 准备依赖。修改依赖时同步更新本项目锁文件。pnpm dev 调试独立网页；pnpm build 生成正式产物；pnpm start 只运行成品。pnpm dev:dsh 只用于已启动的 Wingmate 开发后台，自动监听并使用 .dev/dsh 中的独立产物；不要将它指向日常使用环境。无需为了修改业务插件而重新编译 Wingmate。

在当前 Wingmate 内，使用 wingmate_plugin 的 register 关联绝对源码目录，build 按包名构建并调用官方安装服务。list 查看关联，forget 只移除关联。普通插件启停或卸载继续使用 plugin_manager。界面中的“编译并更新”和工具执行同一逻辑；不要直接覆盖已安装文件。

缺少开发辅助工具时，可以在工作区构建并打包，然后通过官方 plugin_manager 的 install_bundle 安装生成的 tgz；首次自举不依赖辅助插件已经安装。外部智能体可通过指向目标环境的 dsh plugin CLI 安装相同产物。不要手写 Profile 清单，也不要默认安装到另一个用户环境。

## 完成条件

执行类型检查及相关测试，检查实际包内容。观察安装结果 application 和 warnings；restart-required 尚未生效，failed 需要修复，不能以构建成功代替功能验证。验证侧栏入口、功能页、官方详情页，以及禁用和重新启用；后台更新要实际调用新逻辑。编译失败应保留之前安装的版本。报告未完成的运行验证，保留用户数据。
