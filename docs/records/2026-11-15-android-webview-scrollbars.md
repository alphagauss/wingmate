# 2026-11-15 Android WebView 滚动条复核

重新扫描 DSH `0.2.2-rc.1` 发布包的工作台样式，包含打开文件时才加载的表格预览模块。10 处 `scrollbar-width: none` 中，上游为 6 处提供了 `::-webkit-scrollbar` 写法。Wingmate 为目录选择器分栏和路径面包屑保留回退，并为原先漏查的工作表标签栏和公式栏补上回退。主题中的 `scrollbar-width: thin` 已有独立的 WebKit 路径。

上游主分支 `39163dd30f` 仍缺上述四处回退。该分支新增的侧栏滚动区域已自带 `::-webkit-scrollbar` 写法。本次仍使用锁定的 DSH `0.2.2-rc.1`。

在 Redmi Note 10 Pro 的 Android WebView `130.0.6723.107` 上，通过 USB 转发打开远程工作台中的测试 CSV。CDP 确认两个实际控件的滚动条均为 `display: none`，放入超宽内容后两者仍可横向滚动。Android 本地载荷重新构建通过，本次表格滚动的真机验证使用远程版。适配范围和撤销条件见[运行时维护指南](../runtime-maintenance.md#适配与升级记录)。
