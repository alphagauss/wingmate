# Rust 模块

[发行入口](wingmate-launcher/README.md)负责无 GUI 的 CLI、版本协调、资源释放、服务引导与本机集成；[管理服务](wingmate-service/README.md)独立运行，管理已安装内容和业务进程。服务不依赖发行资源或客户端，桌面通过同一引导和管理接口使用本机后台。

窗口与 WebView 能力属于[桌面客户端](../src-tauri/README.md)，服务不依赖桌面系统库。
