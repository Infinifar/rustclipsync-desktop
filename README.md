# RustClipSync Desktop

面向 Windows 11 的原生剪贴板同步客户端：**WinUI 3 + C#/XAML 界面，Rust 常驻同步进程**。
界面按需启动，关闭窗口后退出 UI 进程，Rust 进程继续负责托盘、剪贴板监听和同步。

当前已实现第一阶段的 Rust 后台与 Windows 平台适配，提供开发用 agent；WinUI 页面与 MSIX 安装包后续接入。Windows MSVC 原生测试与开发构建已通过 [CI](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41191068330)。本地目录沿用 `rustclipsync-destop`，远程仓库为 [Infinifar/rustclipsync-desktop](https://github.com/Infinifar/rustclipsync-desktop)（当前私有）。

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
