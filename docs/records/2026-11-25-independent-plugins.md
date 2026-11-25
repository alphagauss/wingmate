# 2026-11-25 独立插件开发与发行验证

本次继续使用锁定的 DSH `0.2.2-rc.1`，将随包插件源码迁至 `bundled-plugins`，为独立项目保留 `plugins`。运行包中的安装路径仍是 `plugins/<名称>`。电脑端新增 `wingmate-devtools`，通过官方插件管理器安装构建成品；独立插件更新不需要重新编译 Wingmate。当前使用约定见[开发辅助插件](../../bundled-plugins/wingmate-devtools/README.md)和[独立插件入口](../../plugins/README.md)。

`dsh-plugin-demo` 提供共用的 TSX 界面和 TS 业务代码、网页与 DSH 入口、独立依赖及开发命令。本次未为示例创建独立 Git。插件详情采用根目录 `README.md`，随安装包分发，删除自定义 `meta.detail`。完整示例同时生成随包模板，配合开发技能供创造模式使用。

## Windows 验证

- 根目录 `pnpm test`、`pnpm build` 和 `pnpm test:smoke` 通过。示例的业务、HTTP、客户端加载及开发输出隔离测试通过；技能格式校验通过。
- 真实 Tauri WebView 中验证示例侧栏、统计页面和 README 详情。禁用插件后入口移除，重新启用后恢复。开发页面关联源码并编译安装成功；故意制造编译错误后，旧版接口仍可调用。匿名请求开发 API 返回 401。
- `pnpm dev:dsh` 更新宿主逻辑与界面时，DSH 进程未重启，正式 `dist` 未变化。退出后监听结束，临时配对会话被撤销。
- 将随包模板复制到仓库之外并改名后，独立安装依赖、构建、测试和安装成功。验证后已移除源码关联并卸载该测试插件。
- 创造模式中可发现 `wingmate-plugin-development` 技能。界面与 Agent 工具共用操作及权限检查的自动化测试通过。
- 单文件发行冒烟验证了开发入口、技能和模板的完整性，并在隔离 PATH 下使用随包 Node、npm、pnpm 构建无依赖测试插件。首次安装、同版本源码更新、源码修改不自动生效、编译失败保留旧版均通过。测试后台和管理服务退出，发行副本及运行数据已由冒烟脚本清理。

已有安装通过 Wingmate 显式重启加载新增随包组件；独立插件后续由开发页面手动编译更新。正常运行不启动源码监听或开发服务器。

## 未覆盖与收尾

本轮未运行 macOS、Linux 或 Android 验收。测试环境未配置模型，因此未验证模型自主完成“创建、开发、安装、更新”的整段对话；不能将技能可见和工具测试等同于真实模型自举验证。

开发进程及本轮监听已停止。仓库外模板测试目录的删除被自动审批连续拒绝，返回 `blocked by policy`，因此保留在 `C:\Users\bornf\AppData\Local\Temp\wingmate-plugin-bootstrap-lJpnMw`。仓库内本轮临时文件的清理也被同样拒绝：`work/tests/devtools-package`、`work/tests/smoke-Qvjs8j`、`work/tests/smoke-ZwhJWf`，以及 `work` 下本轮生成的 `demo-*` 截图和日志、`devtools-format.log`、`final-format.log`、`plugin-*` 调试文件。未尝试其他删除方式。
