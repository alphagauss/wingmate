# Wingmate 执行后端

组合插件通过官方 Web 的侧栏入口选择执行后端。选择保存在当前页面地址，由 DSH 已认证的调用与订阅携带。宿主用同一个 DSH 网关服务接收调用和流，外部工作区及会话在内存中投影。DSH 原生请求继续由原生服务接收。

针对 `0.2.2-rc.1`，插件使用 Web 启动注入和 Connection transport hooks 标记当前页面的请求，宿主适配 Typert 网关的单次调用和流入口。升级 DSH 时验证这两个入口、侧栏选择器和会话流快照。持有 Agent 任务的宿主生命周期与查看页面分离。

工作区、会话和网关投影参考 [ds-harness-remote 7d4d73baee7b](https://github.com/liguobao/ds-harness-remote/tree/7d4d73baee7b/packages/plugin/src)，随包保留 [MIT 许可](LICENSE.remote)。

开发验证可将 `package.json`、`client.js`、`transport.js`、`workbench.mjs` 和 `catalog.mjs` 放入隔离插件目录，把 `test/fixture.mjs` 复制为该目录的 `host.mjs`，然后按[插件验证入口](../README.md#在-wingmate-中验证)加载。用两个已登录同一测试后台的浏览器页面，分别选择 DSH 与 ACP Fixture。后者显示固定工作区、会话及随后到达的一条消息。
