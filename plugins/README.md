# 自用 DSH 插件

本目录承载 Wingmate 自用插件源码。目前尚无自用插件，社区插件的发行接入见[运行时维护指南](../docs/runtime-maintenance.md#社区和自用插件集成)。

插件遵循所选 DSH 的插件与 Profile 机制，避免在 Wingmate 中另建一套业务插件协议。普通前端共享组件归 [packages](../packages/README.md)。

## 开发 DSH 插件

DSH 插件教程与 API 由上游维护。以下是三个官方入口，页面右上角可切换语言。

| 入口 | 何时查阅 |
| --- | --- |
| 入门 · [中文](https://deepseek-harness.github.io/deepseek-harness/guide/quickstart) / [English](https://deepseek-harness.github.io/deepseek-harness/en/guide/quickstart) | 熟悉 Web UI、模型配置和工作区。 |
| 开发 · [中文](https://deepseek-harness.github.io/deepseek-harness/develop/basic/) / [English](https://deepseek-harness.github.io/deepseek-harness/en/develop/basic/) | 编写插件的主线。先做第一个插件，再按需看 Tool、配置、生命周期、服务和事件。需要交付时看[打包与安装插件](https://deepseek-harness.github.io/deepseek-harness/develop/basic/publish)。 |
| 参考 · [中文](https://deepseek-harness.github.io/deepseek-harness/reference/) / [English](https://deepseek-harness.github.io/deepseek-harness/en/reference/) | 按能力查架构、Cordis API、[插件配置目录](https://deepseek-harness.github.io/deepseek-harness/reference/config-catalog)和子系统。涉及界面时查[客户端模块](https://deepseek-harness.github.io/deepseek-harness/reference/subsystems/client-modules)。 |

开发前先从 [运行时依赖声明](../runtime/package.json) 和 [锁文件](../runtime/package-lock.json) 确定目标 DSH 版本。**官方文档可能滞后于源码，两者有冲突时，以目标 DSH 版本对应的源码为准。** 在 [上游仓库](https://github.com/deepseek-ai/deepseek-harness) 切换到对应的发布 tag 或提交，查阅实现和内置插件示例，再用锁定发布包的 CLI、类型声明和实测行为确认。参考检出的归属见 [upstream](../upstream/README.md)。

[dsh-Go 的文本统计工具示例](https://github.com/ccch1mneyyy/dsh-Go/blob/4cc476d29172745ff00255f1035e5de31737af29/docs/plugins/from-zero.md)可补充参考 Tool 注册、输出 schema、普通文本结果和组合包打包。示例中的 `dsh-go/view`、`dshGo` 清单、桥接和 `@dsh-go/plugin-kit` 属于 dsh-Go 的原生界面扩展。Wingmate 当前复用官方 Web，界面扩展查上表的客户端模块。

## 在 Wingmate 中验证

1. 用 `pnpm runtime:dev` 准备锁定版本的安装树。`runtime/stage/current-<版本>` 中的目录名指向本次安装树，其下的 `node_modules/@deepseek-ai/dsh/lib/bin.js` 是 DSH 入口。可用 Node 调用此入口，也可用同版 DSH CLI。
2. 将 `DSH_HOME` 和 `DSH_AGENTS_HOME` 指向本次验证专用目录，按[第一个插件](https://deepseek-harness.github.io/deepseek-harness/develop/basic/)准备 patch，其中的本地插件入口使用绝对路径。用 `dsh web --patch <补丁文件> --dump-config` 核对配置层，再用同一 patch 启动 Web，分别确认插件成功加载和实际功能可用。复用开发安装树时，将命令中的 `dsh` 换成 `node <DSH入口路径>`。
3. 需要交付时，在组合包的 `engines.dsh` 中声明经过验证的兼容范围。先检查打包清单包含 manifest、patch 和实际加载入口，再将生成的 `.tgz` 安装到全新的隔离测试目录。按官方安装指南验证加载、实际功能及禁用或移除，避免开发目录链接掩盖漏打包的文件。涉及 Web 界面的改动要在 Web 中验证。需要在 Wingmate 桌面验证时，可把组合包安装到[开发数据目录](../crates/wingmate-service/README.md#数据与管理边界)的 `web` Profile。随 Wingmate 分发时，另按[运行时维护指南](../docs/runtime-maintenance.md#社区和自用插件集成)记录接入决定，并按[验证约定](../scripts/README.md)验证单文件发行产物。

改代码后如果行为未变，先核对当前 Profile 加载的是源码、构建输出还是已安装的包副本。按需重新构建或安装，再启动隔离的 DSH 进程并重新触发功能。重启 Wingmate 后台会中断正在运行的任务。
