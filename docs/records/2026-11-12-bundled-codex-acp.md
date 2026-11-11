# Codex ACP 随插件交付

2026-11-12，在 Windows x64、Node 24.16.0 验证。

原先仅打包 Wingmate 的 ACP 客户端，适配器依赖主机全局安装。本次将 `@agentclientprotocol/codex-acp@2.6.0` 的已内联 JavaScript 入口与许可证一起放入执行后端插件。npm 发布来源对应 `f3208913693bd4d5171d6c6b9be5ec1ff96c6431`，版本及完整性由插件声明和 pnpm 锁文件保存。[上游构建入口](https://github.com/agentclientprotocol/codex-acp/blob/f3208913693bd4d5171d6c6b9be5ec1ff96c6431/build.mjs)

Codex 本体使用主机安装，默认通过 `codex` 命令启动，支持 `CODEX_PATH`。选择此范围以复用主机的 Codex 版本与更新方式。`WINGMATE_CODEX_ACP` 保留为自定义适配器入口。发行包携带适配器自身的 Apache-2.0 许可及入口内的第三方声明。

`pnpm test`、`pnpm build` 和 `pnpm test:smoke` 通过。单文件释放出的插件可以找到内置适配器，复制出的 ACP 入口在空 PATH、空 Node 模块搜索路径下通过版本检查。真实主机 Codex 与复制出的入口完成 ACP 初始化，隔离的空凭据目录收到预期的认证要求。

模型请求、登录交互与其他操作系统的运行验证仍待覆盖。已有后台通过重新准备运行内容并「重启并更新」切换到携带适配器的插件。
