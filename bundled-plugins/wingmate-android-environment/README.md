# Android 本机 Agent 环境

随 Android 本地版预装，为本机 DSH 提供已随包交付的工具替代方案。Node 的 `fetch` 可发起 HTTP 请求，Python 的 `sqlite3` 和 `zipfile` 可处理 SQLite 数据库与 ZIP 文件。

插件通过 DSH 的 `systemPrompt.section()` 注册固定文本，供同一运行配置下的新会话复用提示词前缀。会随设备或会话变化的事实适合通过 DSH 运行时上下文提供。工具声明和执行权限沿用 DSH。

发布前在新会话比较实际模型请求的 system 消息和工具声明，覆盖同进程与重启后的启动。用同一新会话续跑多轮并查看提供方报告的缓存命中。
