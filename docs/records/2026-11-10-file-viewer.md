# 2026-11-10 文件预览与 LibreOffice 裁剪

本次沿用锁定的 DSH npm 发布包，将 Word、PowerPoint 侧栏预览替换为 File Viewer，并增加常用扩展格式。依赖与补丁的精确版本以配置和锁文件为准，长期规则见 [运行时维护指南](../runtime-maintenance.md)与[插件说明](../../plugins/wingmate-file-viewer/README.md)。

## 相对原版的改动

- 预装插件在组装前显式执行构建，再打包为独立 npm 产物。开发与发行共用插件和补丁组合。
- 配置补丁禁用 `office-to-pdf`。版本化代码补丁移除内置 Office 预览注册，保留原有其他预览实现，避免下拉菜单留下不可用选项。
- 发行归档统一排除 LibreOffice Kit 包、各平台引擎和 WASM 后备运行文件，保留许可证与声明。
- 新增 File Viewer 插件，通过已有授权文件读取与侧栏插槽提供预览，静态资源全部随包交付。当前启用的扩展名见插件格式清单。
- Office skill 未启用，后台 Office 转 PDF、页面渲染和公式重算未作为本次交付能力。File Viewer 的预览版式与 LibreOffice 输出可能不同。

## 体积

Windows x64，同一 DSH 版本和 Node 内置 Zstd 9。单位 MiB，括号内为字节数。

| 产物 | 替换前 | 本次 |
| --- | --- | --- |
| DSH 运行归档 | 114.0（119,529,862） | 55.3（57,951,260） |
| 单文件 EXE | 约 124.0 | 65.3（68,485,120） |

当前规则排除了 788 个 LibreOffice 运行文件、191,653,338 字节的未压缩内容。上表归档已包含新增插件及所选渲染器资源，不能将删除文件的未压缩大小当作归档收益。构建报告位于 `runtime/out/build-report.json`。

## 验证

环境为 Windows x64、Node 24.15.0、WebView2 157.0.4341.42。使用隔离的 DSH、管理目录和 WebView 数据，未使用模型凭据。

- `pnpm test` 通过裁剪、补丁匹配、资源路由、并发与取消测试，以及前端构建和 Rust 测试。
- `pnpm build` 生成单文件 EXE。`pnpm test:smoke` 验证释放、后台生命周期、Web 认证、预装插件、全部静态资源字节、原生图片/语音加载/终端，以及 LibreOffice 运行内容缺失和旧入口移除。
- 在复制出来的真实发行 EXE 中，通过 WebView CDP 点击工作区文件，验证 DOC、DOCX、PPT、PPTX、ODT、EPUB、FB2、UMD、EML、MSG、MP3、OGG、WAV、MP4 共 14 种格式。外部 HTTPS 请求被测试拦截，预览无资源加载错误。音视频样例完成本地解码准备。样例来自 File Viewer 固定提交 `760038d88e9252bc746c04626b82b84226ef6948`，WAV 与性能/异常样例由测试生成。
- EPUB 初次测试发现 Blob 样式被 CSP 阻止，修正后复验通过。打印与 HTML 导出未纳入预览工具栏。
- 补齐嵌套依赖和 Worker 的许可证后再次构建并执行发行冒烟。逐个核对确认 39 个非声明浏览器资源与上述界面验证所用资源完全一致。
- 提交前审核补充插件实际归档的入口、补丁及隔离加载检查，未部署的运行依赖明确报错。修复通用代码补丁对替换文本中 `$` 序列的误解释，在已有测试中加入回归断言；静态路由单测缩减为代表性资源，发行冒烟仍校验全部资源。重新执行 `pnpm test`、`pnpm runtime:prepare` 和 `pnpm test:smoke` 通过，运行归档字节与前次成品一致。

浏览器测试证据保存在 `work/file-viewer-check/`，包括 `ui-results.json`、截图、`stress-results.json`、`cross-platform.json` 和 `final-assets.json`。单文件冒烟每次在独立的 `work/smoke-*` 中保存 `result.json`。这些是本地测量产物。

## 资源回收与异常输入

使用包含 1,500 段文字和 50 个分页符的 DOCX 连续打开、关闭五轮，确认最后一段被渲染。每轮关闭后通过 CDP 显式触发垃圾回收，JS 堆保持约 24.3 MiB。随后再做八轮打开即关闭，最终 DOM 文档数、节点数和事件监听数量与基线相同，分别为 11、1,874、408；JS 堆从 25,382,720 字节变为 25,563,980 字节。

这项测量覆盖该样例和强制回收后的浏览器堆，未测量所有格式的峰值进程内存。iframe 生命周期、两路解析上限和静态资源缓存共同控制额外开销，DSH 自身的原始字节缓存继续由上游管理。

损坏 ZIP/DOCX 显示解析错误并释放 iframe。33 MiB 文件被 DSH 的 32 MiB 读取上限拒绝，未创建预览 iframe。单元测试补充验证排队取消、运行中取消和保留原始 ArrayBuffer。

## 平台与升级边界

对之前下载的 macOS ARM64、Linux AMD64、Linux ARM64 安装树执行实际文件规则检查，分别排除 790、102、102 个 LibreOffice 文件，仅保留各自 5 个许可证或声明文件。三份预览包目标文件的原始哈希均与版本补丁一致。Android 后台这里只按 Linux ARM64 安装布局检查。

macOS、Linux 和 Android 尚未运行验收。FLAC、M4A、WebM 已注册，但本次没有对应媒体样例，实际播放取决于目标 WebView 编解码器。语音只验证原生模块加载，未执行模型推理。开发窗口曾遇到 WebView2 状态错误，界面验收因此使用独立资料目录的真实发行窗口完成。

同一 DSH 版本按新归档指纹释放目录。已有后台继续使用旧内容，用户明确重启后启用新版。此次继续使用原 `target` 编译缓存和 `target/release/wingmate.exe` 输出位置。
