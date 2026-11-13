# 2026-11-12 Android 远程版首包

Windows 11 x64 本机使用 JDK 17、Android SDK 36 和 NDK r27 构建 Wingmate 0.1.0。安装包为 `outputs/android/wingmate-android-arm64-v8a.apk`，包名为 `dev.wingmate.mobile`。

| 项目 | 实测 |
| --- | --- |
| 签名 APK 体积 | 8,653,778 字节（8.25 MiB） |
| 成功构建至签名 | 71.1 秒，使用已缓存的 Rust 和 Gradle 依赖 |
| 其中 Gradle 打包 | 约 67 秒，执行 Kotlin 编译、R8 和 Release 检查 |
| SHA-256 | `f7e03f8be1f9f23ab9b4e7eb5081e1f3498c50eb0abbd440e5a685c858bec802` |

首包使用 Tauri，构建时将 Rust 库复制给 Gradle，并关闭跨盘依赖的 Kotlin 增量编译。首次 Gradle 尝试下载依赖并在 Kotlin 编译处失败，耗时约 11 分 6 秒。修正后沿用缓存完成上表构建。

链接解析测试、Wingmate 网关配对和隧道转发回归均通过。检查成品仅含 `arm64-v8a` 原生库，APK 和 ELF 均满足 16 KiB 对齐。APK 使用本机 Android 测试密钥，v2 和 v3 签名校验通过。

构建机未连接 Android 设备。真机分别用新生成的局域网配对二维码、临时隧道配对链接打开工作台，然后返回连接页。再次选择最近连接，检查配对会话是否仍有效。

## 管理按钮更新包

`outputs/android/wingmate-android-arm64-v8a-management.apk` 的签名后大小为 8,637,394 字节（8.24 MiB），依赖已缓存时构建至签名耗时 36.3 秒。已核验 ARM64、APK 对齐和与首包相同的签名证书。真机待验证拖动位置保存、进出管理页及工作台滚动位置保留。

## Kotlin 增量构建

在同一台机器上复用已编译的 ARM64 Rust 库和 Gradle 缓存，仅测量 Gradle 生成 Release APK 的阶段。无源码变化时重复构建用时 1.3 秒。临时调整管理按钮的一行 Kotlin 样式后，构建用时 15.8 秒，其中 Kotlin 编译任务 0.62 秒，R8 优化 13.50 秒。

Kotlin 构建报告确认仅重新编译了改动的 `ManagementButton.kt`。恢复源码并再次构建时，Kotlin 编译任务命中 Gradle 输出缓存。跨盘的 `tauri-android` 依赖使用非增量 Kotlin 编译，后续构建复用该依赖的任务结果。

## 上游界面与交互对齐包

早期 APK 的首页、配色、四语文案、连接和通知桥接参考或移植自 [deepseek-harness-desktop 的移动端](https://github.com/dsh-tauri/deepseek-harness-desktop/tree/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/apps/native)，适用该提交的 [MIT 文本](https://github.com/dsh-tauri/deepseek-harness-desktop/blob/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/LICENSE)及[附加条款](https://github.com/dsh-tauri/deepseek-harness-desktop/blob/ff5e4815a7d7a7920d4b4dc02af18fdf0ac9e937/LICENSE.details)。

`outputs/android/wingmate-android-arm64-v8a-drawer.apk` 的签名后大小为 8,633,374 字节（8.23 MiB），本机增量构建至签名用时 22.4 秒。与首包使用相同的测试签名。已核验 ARM64、APK 对齐、新版启动图标资源、四种语言资源和工作台脚本，前端和 Android 配对地址解析检查均通过。

| 上游区域 | 本次实现 |
| --- | --- |
| 首页 | 历史连接自动接入、局域网扫描、取消与三点进度动画，扫码和最近连接入口 |
| 扫码 | 相机权限、二维码、无效码重试、手动输入与返回相机 |
| 抽屉 | 右侧滑入并平移底下页面、遮罩点按关闭、当前连接、最近五条、状态、刷新、切换、重命名、删除、断开后扫描 |
| 工作台 | 加载、超时、重试、网页历史、跟随网页明暗主题、任务通知与会话定位 |
| Wingmate 适配 | 自适应启动图标、可拖动圆形悬浮入口、网关身份检查与 Cookie 配对 |

真机待对照图标裁切、抽屉滑动和点击、扫码与手输、局域网发现、工作台主题及通知打开会话。

## Kotlin + WebView 迁移包

`outputs/android/wingmate-android-kotlin.apk` 签名后为 1,493,938 字节（1.42 MiB），比上一版 8.23 MiB 小 82.7%。APK 包名仍为 `dev.wingmate.mobile`，构建号为 1001。与上一版的签名证书一致，v2、v3 签名和 APK 对齐校验通过。包内没有 ABI 专属库，已核对首页、翻译、工作台脚本、启动图标资源和 Release DEX 的首页桥接入口。

同一台 Windows 构建机使用已缓存依赖和 Gradle 配置。计时到 Gradle 生成 APK，采用相同的 Kotlin 文件做临时改动，测完恢复源文件。

| 场景 | 实测 |
| --- | --- |
| Debug 无改动 | 1.2 秒 |
| Debug 修改悬浮按钮的 Kotlin 文件 | 1.9 秒，其中 Kotlin 编译 0.40 秒 |
| Release 无改动 | 4.0 秒 |
| Release 修改悬浮按钮的 Kotlin 文件，两次 | 25.4、49.8 秒，其中 R8 分别用时 23.96、44.93 秒 |
| 恢复源文件并签名，命中 Gradle 输出缓存 | 2.2、5.2 秒 |

Release 本次单文件改动耗时高于上面 Tauri 版的单次测量。第二次仅重新编译了修改的 Kotlin 文件，用时 0.34 秒。提交审核时清理 Android 工程内的构建目录后重新构建并签名。最新 APK SHA-256 为 `9198611c4368d4537f0125a8b85b778c394e12006c86282ab4987623ff59bb06`。

首页桥接的历史迁移、扫码结果回传和配对解析测试通过。构建机未连接 Android 设备，覆盖安装后最近连接、浮钮位置与 Cookie 保留，以及局域网和临时隧道的端到端流程待真机核验。
