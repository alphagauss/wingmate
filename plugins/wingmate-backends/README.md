# Wingmate 执行后端

普通 DSH 组合插件，在侧栏底部选择 DSH 或 Codex。DSH 持续提供 Web、插件和原生 Agent。外部会话的侧栏、历史、实时内容、发送、中断和审批复用官方界面，由 ACP 接入目标 Agent。每个页面独立选择后端，并在当前标签页中分别记住各后端的会话位置。切换页面或关闭窗口时，宿主持有的任务继续。

## 主机依赖

在主机上安装并登录 Codex，再安装 ACP 适配器。普通浏览器与桌面使用相同的主机端适配器。

```sh
npm install -g @agentclientprotocol/codex-acp@2.6.0
```

适配器的 npm 包带有匹配的 Codex，可通过其 `CODEX_PATH` 指向另一个 Codex。登录、设置和原生会话沿用适配器使用的 Codex 用户目录。可用 `WINGMATE_CODEX_ACP` 指定主机上适配器可执行文件或 JavaScript 入口的绝对路径。缺失或启动失败时，后端选择器显示诊断，DSH 可独立使用。

## 数据和能力

目录按原生会话的 `cwd` 分组。新增目录与目录别名保存在 `DSH_HOME/storages/wingmate-backends/codex.json`，默认 `DSH_HOME` 沿用 DSH 的 `~/.dsh`。这份文件只保存目录索引。外部会话及凭据由原生 Agent 保存，DSH 的 SessionStore 与 Workspace 存储保持各自的数据归属。

当前外部会话支持文本输入、历史恢复、流式正文与思考、基础工具结果、一次性审批、中断、模型与权限模式选择，以及 Agent 声明支持的会话改名。图片和通用附件、归档、置顶及全文搜索尚未接入。相关调用报告不支持。模型和权限选项来自 ACP 的会话配置，审批原样选择 Agent 提供的一次性选项。多个页面查看同一会话时，首次有效回答生效，过期回答被忽略。

通用 ACP 客户端、DSH Web 投影和 Codex 的命令发现各自独立。新增后端先验证其 ACP 能力与部署方式，再接入同一投影。Claude 留待后续按需适配。ACP SDK 随宿主产物内联，外部 Agent 适配器由主机安装。发行依赖约定见[运行时维护指南](../../docs/runtime-maintenance.md#打包范围与所有权)。

## DSH 适配与验证

针对 `0.2.2-rc.1`，插件使用 Web 启动注入和 Connection transport hooks 标记当前页面的请求，宿主适配 Typert 网关的单次调用和流入口，沿用其认证和 Peer 生命周期。导航适配将 DSH 的当前会话持久化键映射到各标签页的后端记录。升级 DSH 时验证这两个入口、导航键、侧栏选择器和会话流快照。断线重连重读状态，不重放发送和审批。

工作区、会话和网关投影参考 [ds-harness-remote 7d4d73baee7b](https://github.com/liguobao/ds-harness-remote/tree/7d4d73baee7b/packages/plugin/src)，随包保留 [MIT 许可](LICENSE.remote)。

运行 `pnpm --filter dsh-wingmate-backends test` 检查历史、并发发送、审批关联和页面路由，再按[插件验证入口](../README.md#在-wingmate-中验证)验证真实 Web。早期投影探针 `test/fixture.mjs` 可在隔离目录作为 `host.mjs` 使用，随它复制未打包的工作台模块和客户端。实际运行和发行证据进入有日期的验证记录。
