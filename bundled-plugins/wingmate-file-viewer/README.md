# Wingmate 文件预览

通过 DSH 的 `documentPreviews` 注册表与 `sidebar.right.tab.document` 插槽，把 [Flyfish File Viewer](https://github.com/flyfish-dev/file-viewer) 接入现有侧栏。文档仍由 DSH 的会话文件接口授权读取。依赖版本以本包及根锁文件为准，DSH 兼容范围见 `engines.dsh`。

## 发行范围

随包提供并默认启用 Word、PowerPoint、OpenDocument 文本、电子书、邮件和常用音视频预览。精确扩展名以 [格式清单](src/formats.js) 为准。清单同时参与 DSH 注册和渲染器选择，新增格式需要安装对应模块并验证其资源。PDF、Excel、Markdown、代码和普通图片继续使用 DSH 已有预览。

File Viewer 的 JS、Worker 和声明文件全部由插件构建产物提供，预览无需下载引擎。音视频能否播放还取决于 WebView 的编解码器。浏览器预览的版式、字体与 LibreOffice 转 PDF 可能不同。当前不提供 Office 编辑、公式重算或后台 PDF 转换，Office skill 未启用。预览工具栏提供搜索、缩放和下载等所选渲染器能力，打印与 HTML 导出未纳入本插件。

## 构建与资源

`pnpm --filter dsh-wingmate-file-viewer build` 编译薄客户端入口和独立浏览器模块，按格式生成延迟加载代码，并复制 DOCX、PPTX 所需的离线 Worker。`npm pack` 只携带宿主入口、编译产物和配置补丁，运行时没有 workspace 链接或另行安装的插件依赖。

DSH 原有插件资源路由只支持图片。宿主入口通过现有 `webServer` 注册带内容指纹的专用资源路由，仅响应构建清单中的文件，支持 GET、HEAD 和 ETag。它提供程序静态资源，文档读取仍走 DSH 原有接口。普通浏览器与 Tauri WebView 共用此路径。

## 生命周期与资源取舍

每个可见预览使用独立 iframe，按扩展名载入渲染器。关闭、刷新、切换标签或收起侧栏会移除旧 iframe，让浏览器结束所属 Worker 并释放解析结果。采用此方式是因为各格式解析器对加载中取消的支持不同。重新显示时重新解析，浏览器可以复用静态资源缓存。

全页面最多同时启动两个解析任务，排队期间复用 DSH 的字节。获得解析名额后复制一次字节并转移给 iframe，避免分离 DSH 留存的 ArrayBuffer。解析超过 60 秒结束当前预览并显示错误。文件大小沿用 DSH `workspaceFiles` 的读取上限，原始文件字节的标签缓存仍由 DSH 管理。本插件不保留全局文档缓存，PPT 禁用持久帧缓存，PPTX 使用上游的按需幻灯片渲染，DOCX 使用 Worker 和分批渲染。

iframe 的用途是管理渲染生命周期。它与应用同源，不作为不可信代码执行环境。页面 CSP 阻止联网资源，只允许随包文件及渲染生成的 Blob/Data 资源；外部文档内容的净化沿用渲染器。主题在打开预览时跟随页面，渲染器内部也可切换明暗。

## 与上游的适配

配置补丁禁用 `office-to-pdf`，发行裁剪移除所有平台的 LibreOffice Kit 运行内容。DSH 当前会无条件注册旧 Office 预览，且下拉菜单保留低优先级实现，因此配套 [版本补丁](../../runtime/patches/dsh-0.2.2-rc.1.json) 移除其注册调用。补丁应用与失败策略见 [运行时维护指南](../../docs/runtime-maintenance.md#适配与升级记录)。

升级时核对预览插槽、字节加载、标签可见性、客户端模块和资源服务接口，以及 File Viewer 的 Worker 布局与生命周期。上游提供关闭旧 Office 预览的配置后，撤销代码补丁。仅独立安装此插件的 `.tgz` 可以新增预览，但不会修改 DSH 内置注册代码；Wingmate 发行流程负责完整替换。

移除本插件会失去上述新增预览。恢复原版 Word/PPT 转 PDF 需要同时恢复 LibreOffice 运行内容、启用转换提供方并撤销注册补丁。

## 许可与验证

File Viewer 核心与当前所选渲染器声明 Apache-2.0，依赖包含各自的许可。构建将实际打包模块的许可证、NOTICE 和 Worker 自带的第三方声明放入产物。旧版 PPT 的 `@file-viewer/ppt` 和 PPTX 运行库以编译产物发布，源码公开范围以各包说明为准，不能把整个依赖链视为均有公开开发源码。

Windows Tauri WebView2 的实测记录见 [本次接入记录](../../docs/records/2026-11-10-file-viewer.md)。macOS、Linux 和 Android 使用相同浏览器实现，尚未运行验收。TUI 不提供图形预览。

`pnpm --filter dsh-wingmate-file-viewer test` 验证资源清单、路径限制、缓存响应、解析并发与取消。发行验收还需检查真实侧栏中的文档内容、离线资源、快速切换与关闭，以及 `pnpm test:smoke` 的单文件释放结果。
