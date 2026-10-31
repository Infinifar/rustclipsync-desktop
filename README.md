# RustClipSync Desktop

面向 Windows 11 的原生剪贴板同步客户端：**WinUI 3 + C#/XAML 界面，Rust 常驻同步进程**。
界面按需启动，关闭窗口后退出 UI 进程，Rust 进程继续负责托盘、剪贴板监听和同步。

已提供 WinUI 首次配置窗口、Rust 后台和按用户安装的 EXE 安装器。完整历史/设置页面与 Named Pipe 仍在后续计划中。Windows MSVC 原生测试与开发构建已通过 [CI](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41191068330)。本地目录沿用 `rustclipsync-destop`，远程仓库为 [Infinifar/rustclipsync-desktop](https://github.com/Infinifar/rustclipsync-desktop)（当前私有）。

- [后台使用与构建](agent/README.md)：配对、托盘、配置、状态恢复和当前限制。
- [验证记录](VALIDATION.md)：已执行检查及 Windows 实机待验收项。
- [架构与页面设计](DESIGN.md)：进程边界、共享核心、Windows 适配、安全和界面。
- [实施与验收计划](DEVELOPMENT-PLAN.md)：实施顺序、交付物和测试门槛。
- [本地通信契约草案](IPC.md)：WinUI 与 Rust 的 Named Pipe 消息边界。

共享代码位于相邻项目 [rustclipsync-core](../rustclipsync-core/README.md)。Omaclipsync 已接入本地提取版本；Windows 将使用同一核心，Android 继续用 Kotlin 实现同一协议。

现有生态：

- [rustclipsync-server 服务端](https://github.com/Infinifar/rustclipsync-server)
- [HTTP 接口协议](https://github.com/Infinifar/rustclipsync-server/blob/main/PROTOCOL-V1.md)
- [Omaclipsync Linux 插件](https://github.com/Infinifar/omaclipsync)
- [Android 输入法客户端](https://github.com/Infinifar/fcitx5-android-bibi-keyboard)

产品使用正式名称；HTTP 协议路径中的 `/v1/`、IPC 协议号和 Rust crate 版本各自用于兼容性管理。

## Windows 下载与安装

使用 Windows CI 产物 `rustclipsync-windows-x64-setup` 中的 **RustClipSync-0.1.0-setup-x64.exe**，双击按向导安装；不要把单独的 `rustclipsync-agent.exe` 当作安装程序。安装器提供开始菜单入口、可选桌面快捷方式和可选登录启动，默认不要求管理员权限。

安装完成后打开 RustClipSync，填写 HTTPS 服务端地址、账户和密码，点击“连接并启动”。已配对时可直接“启动同步”；关闭窗口后 Rust 后台继续运行，双击托盘图标可再次打开界面。密码经继承的匿名标准输入管道交给子进程，不出现在命令行、环境变量或配置文件里。

WinUI/.NET 和应用本地 C++ 运行组件随安装包分发。程序、托盘、快捷方式和安装器使用来自 Omaclipsync 的同款图标。安装器和开发构建目前未签名。卸载程序保留 `%LOCALAPPDATA%\RustClipSync` 的配置和加密状态，便于重新安装。

手工构建需要 Windows SDK、.NET 10、Rust MSVC 和 Inno Setup 6：先构建 Rust release，再执行 `./scripts/build-windows.ps1`。CI 还会进行隔离安装、双击首次启动、图标及卸载检查。
