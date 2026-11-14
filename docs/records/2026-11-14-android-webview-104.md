# 2026-11-14 Android 工作台 WebView 104

在 Redmi Note 10 Pro 的 Android 13、Android System WebView 104.0.5112.97 上，用独立包名的本地 Debug 版和真实 WebView 的 CDP 验证 DSH 0.2.2-rc.1。应用将支持门槛从此前验证过的 WebView 96 提高到 104。

- WebView 原生命中 `(width <= 767px)`。DSH 设置页铺满 393 × 838 CSS 像素的工作台，打开、切换分区和关闭均正常，移除桥接中的 CSS 断点改写。
- WebView 已提供 `Array.prototype.findLast`、`Array.prototype.findLastIndex`、`structuredClone`、`AbortSignal.prototype.reason` 和 `AbortSignal.prototype.throwIfAborted`。仅移除这五项的对照中，工作区和输入框正常。移除整个兼容脚本后，本机工作台缺少 `Promise.withResolvers` 和 `AbortSignal.any`，WebSocket 反复重连。远程版首次配对后复现同样的错误，因为远程主机的页面没有加载 Android 载荷里的脚本。两种 APK 改由 [共用的工作台入口](../../apps/mobile/README.md#开发与构建)注入剩余补齐，载荷删除 HTML 注入。
- 手机断开默认网络后，取消回环适配会显示连接异常，保留适配可自动加载工作区和输入框。来宾仍缺 `xdg-user-dir`，继续通过 DSH 配置指定 Documents 路径。

两种 APK 均以独立包名进行真机复验。远程版通过 USB 转发连接 Windows 上的隔离 Wingmate 网关，完成首次配对和重新加载，WebSocket 收到业务数据。本地版覆盖安装后启用了新载荷，来宾 `/root` 和 `/root/.dsh` 的 inode 保持一致。手机在没有默认网络时仍能打开本机工作台。两种工作台的设置页均铺满 390 × 873 CSS 像素的 WebView。

用临时把门槛设为 105 的 Debug 包在同一台手机上验证了版本更新提示和关闭提示后返回首页。正式代码的 104 边界及更旧版本由 JVM 测试覆盖。两种 Release APK 均通过构建、测试签名、16 KiB 对齐和包内容检查。Windows 上需要 POSIX 文件系统的三个运行时测试跳过。

已有安装沿用原 APK 签名和载荷更新机制。此次覆盖升级使用隔离 Debug 安装。远程验证使用 USB 转发，未覆盖实际局域网、公网隧道或真实模型调用。
