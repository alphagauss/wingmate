# 2026-11-17 Android 本机环境插件

继续使用 DSH `0.2.2-rc.1`。电脑端和 Android 本地版各自选择预装插件并组装 Web 启动补丁，共用 Office 转换禁用规则。电脑端的桌面与无 GUI 构建使用同一套组合。Android 本地版新增环境插件，通过固定系统提示词说明随包工具的替代用法。详见[运行时维护指南](../runtime-maintenance.md#社区和自用插件集成)和[插件说明](../../bundled-plugins/wingmate-android-environment/README.md)。

Windows 无 GUI 单文件发行构建及冒烟通过，覆盖后台服务、执行后端、文件预览和运行目录复用。WSL 构建的 ARM64 载荷通过归档哈希检查，基础环境身份与本机前一份载荷一致。rootfs XZ 增加 5,512 字节。

Android 本地及远程 Debug APK 通过签名与对齐检查，两种 Release APK 均构建成功并核对了载荷。本地 APK 的归档按清单校验，只包含两个预装包，远程 APK 无本地载荷。用 QEMU 启动归档里的 ARM64 DSH Web，以本地测试适配器捕获 standard preset 的模型请求。在同一进程内创建新会话、续聊并重启后创建新会话，system 内容及 31 项工具声明的顺序一致。用加入插件前的组合创建的测试会话可在新组合下继续对话。ADB 未发现设备，此次未测真机安装和服务方缓存命中。
