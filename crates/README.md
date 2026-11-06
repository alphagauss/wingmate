# Rust 模块

承载独立 Rust 库，当前只有 [后台服务](wingmate-service/README.md)。按实际职责和复用边界拆分，拆库不意味着新增独立发行程序。

共享业务能力不依赖桌面窗口；窗口相关能力属于 [桌面宿主](../src-tauri/README.md)。
