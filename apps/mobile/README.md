# Wingmate Android

连接页由 Kotlin 加载 APK 内的 HTML、CSS 和 JavaScript。扫码与粘贴都从扫码页进入，连接前检查 Wingmate 网关标识。配对和工作台在独立的 Android WebView 打开。配对会话由网关 Cookie 保存，最近连接保存主机地址和用户命名。

首页沿用上游的「自动扫描局域网」「扫码连接」「最近连接」。自动扫描先检查历史主机，再探测上游默认端口和历史局域网端口。第一次连接使用随机端口的 Wingmate 主机可扫码或粘贴配对链接。

工作台的半透明圆形按钮可拖动，点按从右侧平移打开连接抽屉。点击工作台遮罩、关闭按钮或按返回键收起抽屉，保留当前工作台。抽屉可扫码、查看和刷新当前及最近连接、点按切换、长按重命名或删除。断开当前连接后按上游方式扫描其他主机。服务和任务由连接的主机管理。首版范围和真机验收条件见 [C2](../../ARCHITECTURE_PLAN.md#c2-android-远程版)。

包体积、构建计时和验证结果见 [Android 远程版记录](../../docs/records/2026-11-12-android-remote.md)。

## 工程布局

按 dsh-Go 的 `app/android` 组织 Android 工程。`android/app/src/main/kotlin/dev/wingmate/mobile/` 下，`MainActivity` 装配首页，`launch/` 承载首页桥接，`remote/` 管理远程连接、扫码和抽屉，`web/` 承载工作台，`i18n/` 和 `theme/` 提供文案与外观。网页资源放在相应的 `assets/launch/` 和 `assets/web/`。

本地 Agent 接入时，运行时安装、进程管理和前台服务对应 dsh-Go 的 Kotlin `runtime/`，移植到同名包下。安装器、进程和服务仍分别由 `RuntimeInstaller`、`DshProcess` 和 `DshService` 承担。dsh-Go 的 `RuntimePlugin` 绑定 Flutter 平台通道，移植时需把它调用的控制器接到 Android 入口。工作台接收内部入口给出的 HTTP(S) 地址，远程网关身份校验留在 `remote/`，本地服务可以从自己的入口交给同一工作台。运行时引用的 `phone`、`performance` 和本地化资源按实际需要移植或适配，构建载荷与许可证按 [D1](../../ARCHITECTURE_PLAN.md#d1-android-本地运行版)核对。

## 开发与构建

安装 JDK 17、Android SDK Platform 36、Build Tools 36 和 Platform Tools，并配置 `JAVA_HOME`、`ANDROID_HOME`。从仓库根目录运行 Gradle Wrapper。日常改动可用 Debug 构建，连接手机后用 `installDebug` 安装。

```powershell
.\apps\mobile\android\gradlew.bat -p apps/mobile/android :app:assembleDebug
.\apps\mobile\android\gradlew.bat -p apps/mobile/android :app:assembleRelease
node --test apps/mobile/test/*.test.mjs
```

Release 产物为 `android/app/build/outputs/apk/release/app-release-unsigned.apk`，用 Android SDK 的 `apksigner` 签名后可安装。远程版只包含 DEX 和资源，同一个 APK 可用于 ARM64 等 Android 设备。同一应用的覆盖安装要求沿用签名。已保存的连接与浮钮位置使用原来的应用数据，首页保留原有本地来源，用于读入早期 APK 的连接历史。

本机测试包沿用 `assembleDebug` 生成的默认测试密钥。签名命令从仓库根目录运行。

```powershell
$releaseDir = 'apps/mobile/android/app/build/outputs/apk/release'
$apkSigner = Join-Path $env:ANDROID_HOME 'build-tools/36.0.0/apksigner.bat'
& $apkSigner sign --v4-signing-enabled false --ks "$env:USERPROFILE/.android/debug.keystore" --ks-key-alias androiddebugkey --ks-pass pass:android --out "$releaseDir/wingmate-android.apk" "$releaseDir/app-release-unsigned.apk"
```

Android Gradle 复用配置和任务输出，Kotlin 使用增量编译。网页直接随资源打包，Node 只用于运行上述 JavaScript 测试。

## 上游来源

从 deepseek-harness-desktop [`ff5e4815` 的移动端](https://github.com/dsh-tauri/deepseek-harness-desktop/tree/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/apps/native)移植了页面布局、主题配色、四种语言、发现与连接交互，以及工作台通知脚本。连接页使用 HTML/CSS，扫码和抽屉由 Android 控件实现。工作台脚本只接收已连接来源的主页面事件。主机身份与配对沿用 Wingmate 网关协议。构建输入均来自本仓库和声明的依赖。

上游 [许可证](android/app/src/main/assets/launch/UPSTREAM-LICENSE.txt)和[禁止商业二次开发的附加条款](android/app/src/main/assets/launch/UPSTREAM-LICENSE.details.txt)随应用资源一并打包。
