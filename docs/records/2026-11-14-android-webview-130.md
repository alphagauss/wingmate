# 2026-11-14 Android 工作台 WebView 130

Redmi Note 10 Pro（Android 13、ARM64）从 Android System WebView `104.0.5112.97` 升级到 `130.0.6723.107`。安装前用 APK 签名工具核对 WebView 和 Trichrome Library 各自的证书与手机预装包一致，并核对静态库版本。升级后由 Android 的 WebView 更新服务和应用 WebView 的 CDP 分别确认生效版本。

- 升级前用两份独立包名的 APK 验证 WebView 104 的更新提示。本地引擎仍能准备和启动。版本判断的单元测试覆盖 129、130 和 131，以及缺失版本的情况。
- 在 WebView 130 上，远程版通过 USB 转发配对，本地版通过回环地址打开 DSH `0.2.2-rc.1`。两种工作台重新加载时 WebSocket 均收到业务数据，也都能选择目录注册工作区、打开测试 CSV 并显示单元格。远程时区菜单限制为 420 CSS 像素，长列表可在菜单内滚动。
- 控件探针覆盖 `:has()`、带 `of` 的位置选择器、`@container`、`cqw`、颜色函数、`@scope`、入场动画和 `scrollend`。数组接口、`URL.canParse()`、`Promise.withResolvers()`、`AbortSignal.any()` 和 `dsh-resource://file/…` 主机名解析均走原生实现。Windows Chromium 130 同样通过目录选择、时区菜单及 CSV 预览。
- Android WebView 130 会拒绝 `scrollbar-width`，Chromium 中 157 和 158 的 WebView 源码也显式关闭该属性。目录选择器的分栏和路径面包屑补上 `::-webkit-scrollbar` 样式后，两种工作台均复测滚动条隐藏，分栏仍可横向滚动，本地版通过覆盖安装更新载荷。[Chromium 源码](https://chromium.googlesource.com/chromium/src.git/+/8eb52611bcb7d11aae6884aeda43b5290e9f0f3c/android_webview/browser/aw_field_trials.cc)

现有安装通过 APK 更新工作台门槛和本机载荷。远程页由连接的 Wingmate 主机提供，随主机运行时更新。未测试 Wi-Fi 直连、公网隧道或真实模型调用。
