# Wingmate Android

连接页由 Kotlin 加载 APK 内的 HTML、CSS 和 JavaScript。扫码与粘贴都从扫码页进入，连接前检查 Wingmate 网关标识。配对和工作台在独立的 Android WebView 打开。配对会话由网关 Cookie 保存，最近连接保存主机地址和用户命名。

首页沿用上游的「自动扫描局域网」「扫码连接」「最近连接」。自动扫描先检查历史主机，再探测上游默认端口和历史局域网端口。第一次连接使用随机端口的 Wingmate 主机可扫码或粘贴配对链接。

点按工作台的可拖动浮动按钮，从右侧滑入全屏应用设置。点顶部返回、按系统返回键或右滑，从子页回到设置首页，再退回原工作台。设置首页的「当前连接」显示远程主机名或「本机工作台」，点按可切换到已保存的远程主机、扫码或手输新地址，或打开已就绪的本机工作台。右上角可直接扫码。启动页的「最近连接」也进入主机列表，支持刷新、长按重命名或删除。本机和远程都可断开。断开后停在「切换主机」页，由用户选择下一次连接，返回时显示启动页。断开保留配对与连接记录，主机上的服务和任务继续运行。验收范围见 [D2](../../ARCHITECTURE_PLAN.md#d2-android-全屏应用设置)和 [C2](../../ARCHITECTURE_PLAN.md#c2-android-远程版)。

应用设置管理 APK 语言、任务通知、系统权限、启动时连接上次主机及诊断。模型、Agent 预设、主题和插件由当前主机的 DSH 工作台管理。手机窄屏使用 DSH 工作台自带的全屏设置页。对不支持 CSS 媒体查询范围语法的 WebView，工作台桥接将 DSH 设置页断点还原为 `max-width` 写法。升级 DSH 时核对设置样式的标识和布局断点。

两种 APK 默认启动到连接首页。纯远程版开启「启动时连接上次主机」后，只尝试最近使用的远程主机。本地版的「开 App 时自动启动」控制已安装的引擎是否随应用启动。「启动时连接上次主机（本机优先）」控制是否自动打开工作台，优先使用已就绪或正在自动启动的本机引擎，其余情况尝试最近使用的远程主机。两个开关默认关闭。连接或本机启动失败时留在首页。本地版首页在引擎就绪时提供「本机工作台」快捷入口，其余状态显示「本机引擎」管理入口。

从连接首页按系统返回退出后，再点桌面图标会重新应用启动设置。按 Home 或从最近任务切回会恢复原页面。启动时选择本机且引擎尚未就绪时，工作台启动页先显示「正在启动 DSH」，引擎就绪后在同一页显示「正在进入 DSH」。等待期间可按返回回到连接首页，引擎继续启动。

包体积和验证结果见 [Android 远程版记录](../../docs/records/2026-11-12-android-remote.md)及 [Android 本地版首包记录](../../docs/records/2026-11-12-android-local.md)。本地工作台已在真机验证的最低版本组合见[工作台连接和默认工作区修复](../../docs/records/2026-11-12-android-local.md#工作台连接和默认工作区修复)。

## 工程布局

Android 工程沿用 Gradle 默认源码集，应用 Kotlin 源码在 `android/app/src/main/kotlin/` 下按功能组织，包名由源码声明。应用设置使用原生界面，布局参考 [dsh-Go 设置页](https://github.com/ccch1mneyyy/dsh-Go/tree/be885805dd69c66926970140f7f9fd15167bbec8/app/lib/ui/settings)。设置和工作台共用 Activity，保留原 WebView 和会话状态，颜色跟随当前 DSH 工作台主题。

`android/local-agent/` 提供本地运行时的安装、进程和前台服务，只有本地版依赖该模块。本机引擎页提供安装、启动、停止、重启、日志及后台运行指引。运行就绪后，应用通过 DSH 的回环地址和登录令牌打开工作台。返回首页、关闭工作台和切换远程连接都不会停止本机进程。

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

从 deepseek-harness-desktop [`ff5e4815` 的移动端](https://github.com/dsh-tauri/deepseek-harness-desktop/tree/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/apps/native)移植了首页、主题配色、四种语言、发现与连接交互，以及工作台通知脚本。连接首页使用 HTML/CSS，扫码和远程主机管理由 Android 控件实现。工作台脚本只接收已连接来源的主页面事件。主机身份与配对沿用 Wingmate 网关协议。构建输入均来自本仓库和声明的依赖。

上游 [许可证](android/app/src/main/assets/launch/UPSTREAM-LICENSE.txt)和[禁止商业二次开发的附加条款](android/app/src/main/assets/launch/UPSTREAM-LICENSE.details.txt)随应用资源一并打包。
