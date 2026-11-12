# Wingmate Android

连接页由 Kotlin 加载 APK 内的 HTML、CSS 和 JavaScript。扫码与粘贴都从扫码页进入，连接前检查 Wingmate 网关标识。配对和工作台在独立的 Android WebView 打开。配对会话由网关 Cookie 保存，最近连接保存主机地址和用户命名。

首页沿用上游的「自动扫描局域网」「扫码连接」「最近连接」。自动扫描先检查历史主机，再探测上游默认端口和历史局域网端口。第一次连接使用随机端口的 Wingmate 主机可扫码或粘贴配对链接。

工作台的半透明圆形按钮可拖动，点按从右侧平移打开连接抽屉。点击工作台遮罩、关闭按钮或按返回键收起抽屉，保留当前工作台。抽屉可扫码、查看和刷新当前及最近连接、点按切换、长按重命名或删除。断开当前连接后按上游方式扫描其他主机。服务和任务由连接的主机管理。首版范围和真机验收条件见 [C2](../../ARCHITECTURE_PLAN.md#c2-android-远程版)。

手机窄屏使用 DSH 工作台自带的全屏设置页。对不支持 CSS 媒体查询范围语法的 WebView，本地和远程工作台共用的桥接脚本将设置页断点还原为 `max-width` 写法。升级 DSH 时核对设置样式的标识和断点。

包体积和验证结果见 [Android 远程版记录](../../docs/records/2026-11-12-android-remote.md)及 [Android 本地版首包记录](../../docs/records/2026-11-12-android-local.md)。本地工作台已在真机验证的最低版本组合见[工作台连接和默认工作区修复](../../docs/records/2026-11-12-android-local.md#工作台连接和默认工作区修复)。

## 工程布局

按 dsh-Go 的 `app/android` 组织 Android 工程。`android/app/src/main/kotlin/dev/wingmate/mobile/` 下，`MainActivity` 装配首页，`launch/` 承载首页桥接，`remote/` 管理远程连接、扫码和抽屉，`web/` 承载工作台，`i18n/` 和 `theme/` 提供文案与外观。网页资源放在相应的 `assets/launch/` 和 `assets/web/`。

`android/local-agent/` 提供本地运行时的安装、进程和前台服务，只有本地版依赖该模块。Kotlin 源码放在模块的 `src/main/kotlin/`。本机入口由首页和连接抽屉打开，可以启动、停止和查看日志。运行就绪后，应用通过 DSH 的回环地址和登录令牌打开同一个工作台。返回首页、关闭工作台和切换远程连接都不会停止本机进程。

首次安装从 APK 释放完整环境。之后在基础环境身份相同的情况下，只更新 DSH、Node 和应用管理的文件，保留 Linux 环境、Python 用户包、会话和工作区。基础环境不兼容时报告原因，保留现有目录。载荷构建和移植来源见 [Android 本地运行载荷](../../runtime/android/README.md)，真机验收范围见 [D1](../../ARCHITECTURE_PLAN.md#d1-android-本地运行版)。

## 开发与构建

安装 JDK 17、Android SDK Platform 36、Build Tools 36 和 Platform Tools，并配置 `JAVA_HOME`、`ANDROID_HOME`。从仓库根目录运行 Gradle Wrapper。远程版可直接构建，本地版先运行 `pnpm runtime:android`。连接手机后可分别用 `:app:installRemoteDebug` 和 `:app:installLocalDebug` 安装。

```powershell
.\apps\mobile\android\gradlew.bat -p apps/mobile/android :app:assembleRemoteRelease
.\apps\mobile\android\gradlew.bat -p apps/mobile/android :app:assembleLocalRelease
node --test apps/mobile/test/*.test.mjs
```

Release 产物分别在 `android/app/build/outputs/apk/remote/release/` 和 `android/app/build/outputs/apk/local/release/`，用 Android SDK 的 `apksigner` 签名后可安装。远程版延用 `dev.wingmate.mobile`、`targetSdk 36`，APK 只包含 DEX 和资源。本地版使用 `dev.wingmate.mobile.local`、`targetSdk 28` 和 ARM64 原生库，可与远程版并装。连接历史、Cookie 与本机运行数据归各自包名所有。同一应用覆盖安装需要沿用签名，远程版首页继续读入早期 APK 的连接历史。

本地 APK 的安装门槛是 Android 8.0（`minSdk 26`）和 ARM64。

本机测试包可以使用 Debug 构建生成的默认测试密钥。签名命令从仓库根目录运行。

```powershell
$releaseDir = 'apps/mobile/android/app/build/outputs/apk/remote/release'
$apkSigner = Join-Path $env:ANDROID_HOME 'build-tools/36.0.0/apksigner.bat'
& $apkSigner sign --v4-signing-enabled false --ks "$env:USERPROFILE/.android/debug.keystore" --ks-key-alias androiddebugkey --ks-pass pass:android --out "$releaseDir/wingmate-android.apk" "$releaseDir/app-remote-release-unsigned.apk"
```

Android Gradle 复用配置和任务输出，Kotlin 使用增量编译。网页直接随资源打包。Android 本地模块的 JVM 测试入口为 `:local-agent:testDebugUnitTest`。

Debug 构建的工作台可通过 ADB 转发 WebView DevTools 端口，检查前端异常和 WebSocket 状态。

## 上游来源

从 deepseek-harness-desktop [`ff5e4815` 的移动端](https://github.com/dsh-tauri/deepseek-harness-desktop/tree/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/apps/native)移植了页面布局、主题配色、四种语言、发现与连接交互，以及工作台通知脚本。连接页使用 HTML/CSS，扫码和抽屉由 Android 控件实现。工作台脚本只接收已连接来源的主页面事件。主机身份与配对沿用 Wingmate 网关协议。构建输入均来自本仓库和声明的依赖。

上游 [许可证](android/app/src/main/assets/launch/UPSTREAM-LICENSE.txt)和[禁止商业二次开发的附加条款](android/app/src/main/assets/launch/UPSTREAM-LICENSE.details.txt)随应用资源一并打包。
