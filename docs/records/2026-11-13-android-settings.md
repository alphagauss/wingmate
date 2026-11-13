# 2026-11-13 Android 应用设置

在 Windows 使用 JDK 17、Android SDK 36 和 Gradle Wrapper 分别构建、签名两个 Release APK。WSL Ubuntu 24.04 生成本地 ARM64 载荷。两包通过 v2、v3 签名和 16 KiB 对齐检查。远程版的归档和清单均无本地载荷、原生库或本机服务。本地版包含四个 ARM64 原生库，内置 XZ 与清单哈希一致。

| 产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,506,551 | `ab79397a1e6a4ba3bb701d9960b3608c4b4e33b68fbd58239f1cdea8edd0e5e2` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,328,719 | `4e500bdc4bb86c8e0dab275e83191b34adab2bb739721b27fb58f46cd06f02bf` |

Android 12、WebView 96 的 Redmi Note 10 Pro 通过 USB 验证。远程 Debug 包通过临时公网隧道配对并进入 DSH。本地 Debug 验收包使用独立包名，验证首次释放、启动、重启、停止、后台前台服务与本机工作台。原有本地 APK 使用不同签名，独立安装保留了其应用数据。

真机检查了设置进出、右滑层级、输入草稿保留、DSH 深浅主题同步、当前连接卡和本机引擎状态。设置首页右上角打开了扫码页，本地验收包从当前连接卡切换到已保存远程主机，并切回本地 DSH，期间本机 Node PID 保持一致。实际模型调用、跨设备二维码配对和长期后台存活仍待专项验证。

## 主机选择与启动开关

两种 Release APK 通过 USB 覆盖安装，设备上的 APK 哈希与构建产物一致。真机验证本机和远程断开后均停在「切换主机」，原工作台可手动重连，本机 Node 在断开和切换远程时继续运行。远程场景使用两个临时网关身份端点，验证关闭自动连接时停在首页、开启后只连接上次主机，以及上次主机不可达时留在首页。

本地版真机冷启动验证了两个开关。只开启「开 App 时自动启动」时，引擎就绪后首页显示「本机工作台」。两个开关均开启时，即使保存的远程端点可达，也优先打开本机 DSH 登录界面。手动断开后继续停在主机列表。

| 最新产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,506,551 | `c7fe0cc8c8875fd86732796d2f7084cdec0e1025eedc7f6744bf2583890427d5` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,328,719 | `be31239f98a7d5160e55d60450c674e6829a6ebec09a207a30fd63fc6e71020f` |

## 从桌面重新打开与本机启动页

在同一台手机上通过桌面图标验证。两个启动开关开启时，冷启动先显示「正在启动 DSH」，引擎就绪后进入本机工作台。按 Home 再点图标恢复同一个工作台页面。按系统返回依次回到连接首页和桌面，再点图标自动打开本机工作台，Node PID 保持一致。手动断开继续显示主机列表，可再次选择本机工作台。

| 最新产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,506,551 | `a78fd8ce8e37a3ccf9494a65de09ca81badf8fd46e1d6dda88b176cdb3ccd8a1` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,328,719 | `4be9204c6c17e4365e74935f80b2a898c4cb7be3fbc377f3cabd4313149e3b36` |

## 本机工作台启动页连续显示

真机冷启动依次观察到「正在启动 DSH」、「正在进入 DSH」和 DSH 工作台，全程使用同一个工作台 Activity。等待引擎时按返回可回到连接首页，引擎就绪后可从首页手动进入。两种 Release APK 均通过签名、对齐和设备安装哈希检查。

| 最新产物 | 字节 | SHA-256 |
| --- | ---: | --- |
| `outputs/android/wingmate-remote.apk` | 1,506,551 | `7816942eda5a1fa6615c98343776b22364e917e3971d729e8f07bfe63f6d59a4` |
| `outputs/android/wingmate-local-arm64-v8a.apk` | 133,328,719 | `be68dc07f07eb1ee85eda0f76f2095649e9bb36816543cd651fb97a748559ead` |
