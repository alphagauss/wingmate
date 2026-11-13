# 2026-11-13 Android 应用设置

在 Windows 使用 JDK 17、Android SDK 36 和 Gradle Wrapper 分别构建、签名两个 Release APK。WSL Ubuntu 24.04 生成本地 ARM64 载荷。两包通过 v2、v3 签名和 16 KiB 对齐检查。远程版的归档和清单均无本地载荷、原生库或本机服务。本地版包含四个 ARM64 原生库，内置 XZ 与清单哈希一致。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,506,551 | `ab79397a1e6a4ba3bb701d9960b3608c4b4e33b68fbd58239f1cdea8edd0e5e2` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,328,719 | `4e500bdc4bb86c8e0dab275e83191b34adab2bb739721b27fb58f46cd06f02bf` |

Android 12、WebView 96 的 Redmi Note 10 Pro 通过 USB 验证。远程 Debug 包通过临时公网隧道配对并进入 DSH。本地 Debug 验收包使用独立包名，验证首次释放、启动、重启、停止、后台前台服务与本机工作台。原有本地 APK 使用不同签名，独立安装保留了其应用数据。

真机检查了设置进出、右滑层级、输入草稿保留、DSH 深浅主题同步、当前连接卡和本机引擎状态。设置首页右上角打开了扫码页，本地验收包从当前连接卡切换到已保存远程主机，并切回本地 DSH，期间本机 Node PID 保持一致。实际模型调用、跨设备二维码配对和长期后台存活仍待专项验证。
