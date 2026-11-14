# 2026-11-14 Android 工作台 CSS 兼容

基于 DSH 0.2.2-rc.1 的发布包，Android 本地和远程 APK 加入共用的工作台兼容层。两种 APK 部署同一份脚本和许可证，兼容代码只在主页面缺少相关能力时启用。目录选择器和菜单仍保留[共享运行时的回退](2026-11-14-directory-picker-webview-104.md)。

在 Redmi Note 10 Pro 的 WebView 104.0.5112.97 上，用独立包名验证本地与远程工作台。远程端通过 USB 转发完成配对，目录路径编辑时恢复外框，时区列表可在 420 像素高的菜单内滚动。本地版实际选择目录并添加为工作区。

两种工作台均运行 CSS 和滚动探针，核对动态 `:has()`、带 `of` 的位置选择器、命名容器查询、`cqw` 随容器宽度变化、sRGB 混色、表格作用域和 `scrollend`。入场动画在真机通过独立样式探针验证。Edge 157 使用相同的探针验证原生行为，兼容层未启用。Android Debug 构建和相关 JavaScript、Gradle 单元测试通过。

表格样式范围和聊天滚动事件使用控件探针验证。容器查询补齐期间，真机日志偶发 `ResizeObserver loop limit exceeded`，布局随后稳定，已验证的页面和探针可继续运行。容器查询库将此列为预期现象。未运行实际模型调用。[容器查询库的说明](https://github.com/GoogleChromeLabs/container-query-polyfill#resizeobserver-loop-errors)

实际 CSV 初次在 WebView 104 上提示「文件资源服务不可用」。同一开发后台和文件在 WebView2 157 的直连、配对入口均能打开。真机 CDP 确认 `dsh-resource://file/…` 的 `hostname` 为空。加入共享运行时回退、重启后台后，用同一手机测试包和配对入口重新打开该文件，CSV 表格已挂载，能读取单元格内容。此项实测覆盖远程版，本机载荷复用同一运行时补丁。

提交前审核修正了 `:has()` 在文档根元素上的匹配与属性变化更新。Edge 157 分别按原生和强制启用回退的路径检查根元素、动态属性及包含 `:has()` 的位置选择器。Windows 单文件发行冒烟通过，Android 本地载荷重新生成，两种 Release APK 均通过构建和资源检查。
