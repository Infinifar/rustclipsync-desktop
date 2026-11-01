# Windows 安装与运行环境

安装包面向 Windows 11 x64，按当前用户安装，使用系统共享运行库。
所需运行库为 **.NET 10 Runtime x64** 和 **Windows App Runtime 2.6.4 x64**
（或启动器接受的同系列兼容更新）。已有兼容版本时无需重复安装。

首次打开时缺少运行库，微软启动器会显示缺失提示和下载入口。
安装完成页也提供两个官方下载按钮；开始菜单中的“RustClipSync 运行环境”
可以在应用无法打开或离线时查看说明。

- [.NET 10 官方下载页](https://dotnet.microsoft.com/zh-cn/download/dotnet/10.0)：选择 x64 Runtime；已有 .NET 10 Desktop Runtime 或 SDK 也可以。
- [Windows App Runtime 2.6.4 x64 官方安装器](https://aka.ms/windowsappsdk/2.6/2.6.4/windowsappruntimeinstall-x64.exe)。
- [Windows App Runtime 官方下载列表](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/downloads)。

Windows 11 不保证预装这些指定版本。.NET Framework、.NET 8/9、x86 运行库，
以及不兼容系列的 Windows App Runtime 都不能代替当前依赖。
运行库安装可能需要管理员权限；应用只打开官方下载链接，不会在后台执行安装器。
离线部署前请提前下载所需运行库。首次补齐依赖的下载量不计入客户端安装包大小。

Rust 后台独立运行；界面关闭后同步继续。账户、加密状态和历史位于
`%LOCALAPPDATA%\RustClipSync`，安装运行库无需重新配对。

## 构建验证

统一 CI 在临时 Windows 桌面验证缺失 .NET 的原生下载提示、缺少 Windows Runtime
时的提示，以及从微软官方下载并校验签名的共享运行库安装。
随后验证实际 WinUI、剪贴板、快捷键、小窗、安装器下载按钮及卸载。
若构建机已预装 2.x Windows Runtime，缺失该运行库的负向测试会明确标记跳过，
不会卸载构建机已有的共享框架。

打包脚本拒绝携带 .NET、ONNX、DirectML 和 WinUI 原生运行库，生成
`size-report.json`，并限制安装器不超过 20 MiB。精简安装包不会按同一比例
降低运行内存，实际占用取决于已打开的页面和数据量。

## 界面资源管理

启动时只创建主页；外观、快捷键、设置以及主页历史在首次打开时创建，随后复用。
主窗口最小化或隐藏时停止状态轮询，恢复后立即刷新，Rust 后台继续同步。
独立历史窗口最小化时也暂停刷新；敏感收件小窗独立更新。

历史页面按存储实例标识与修订号缓存，同一查询无变化时不重复请求或重建列表。
搜索、分页、收藏、删除和自动清理会按实际变更刷新；后台分页只复制当前页预览，
读取单项只复制该项正文，无变化的定时清理不复制整个历史。
