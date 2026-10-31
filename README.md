# RustClipSync Desktop

Windows 11 原生剪贴板同步客户端，使用 **WinUI 3 / C# / XAML 界面与 Rust 常驻后台**。关闭窗口后，已启动的同步继续运行；双击托盘图标可重新打开。

![图标](assets/omaclipsync.svg)

## 页面与功能

布局参考 [Omaclipsync](https://github.com/Infinifar/omaclipsync)，使用 Windows 原生导航、按钮、输入框和主题控件。

| 页面 | 功能 |
| --- | --- |
| 主页 | 实际连接状态、服务端、设备身份、最近同步、待办；发送、接收、暂停/恢复、退出后台；双向/仅发送/仅接收；敏感收件区与历史 |
| 外观 | 跟随系统、浅色、深色主题；标准/较大/大字号，立即应用并保存 |
| 设置 | 服务端与账户配对、敏感策略、正则过滤及测试、备用检查间隔、历史条数/期限、登录自启、关于及脱敏诊断 |

普通历史支持搜索、分页、复制、收藏、删除及清空。收藏最多 50 条，保留至手动删除；普通历史默认最多 50 条、保留 7 天，可调整为 1–1000 条和 1–365 天。历史默认关闭，单条最多 1 MiB、总正文最多 16 MiB，超限不会回滚已完成的同步。

同步使用共享 [rustclipsync-core](https://github.com/Infinifar/rustclipsync-core)。消息由 UUID、epoch、revision 与持久化游标控制，离线只同步最后一条，清空剪贴板或重启不会重放已消费内容。手动接收也不会恢复已经消费的旧值。首次启动不上传已有剪贴板。当前仅支持文本，最大 10 MiB，不支持旧 SyncClipboard 协议。

## 安装与升级

使用 [Windows CI](https://github.com/Infinifar/rustclipsync-desktop/actions/workflows/windows.yml) 中通过全部检查的 `rustclipsync-windows-x64-setup` 产物，解压并运行 **RustClipSync-0.2.0-setup-x64.exe**。产物内包含 SHA256SUMS 与实际运行截图。当前为未签名开发安装包。

按用户安装，默认无需管理员权限，内含 .NET、WinUI 和应用本地 C++ 运行组件。安装后从开始菜单打开，进入“设置 → 服务端与账户”，填写 HTTPS 根地址、用户名和密码，点击“连接并启动”。

已有 0.1.0 配对可以直接覆盖升级：配置、DPAPI 密钥、设备身份和同步游标会保留。不要删除数据目录来解决连接问题。改变账户或重新输入密码会请求确认重新配对，该操作会放弃待发送内容与当前敏感收件项。

## 开机自启

在 **设置 → 开机自启与后台** 中启用“登录 Windows 后自动启动同步”。程序管理当前用户启动文件夹里的 `RustClipSync.lnk`，目标为后台的 `autostart` 模式；登录后只启动后台，并保留暂停状态，不打开主窗口。

关闭开关会移除该应用的启动项，不停止当前同步。自启统一由应用设置管理，安装器不另外保存一份开关。Windows 的启动应用设置和组织策略可能限制实际启动，可使用页面内按钮打开系统设置查看。卸载会清理该启动项，同时保留 `%LOCALAPPDATA%\RustClipSync` 的配对与加密数据。

## 隐私与安全

- **同步已标记的敏感文本**默认关闭。部分应用可能不标记敏感内容，因此不能识别所有敏感信息。
- 开启后，收到的敏感文本只进入应用内的私有收件区，不自动写系统剪贴板；关闭历史时仍可使用当前收件项。用户主动复制时附带 Windows 历史/云同步排除标记，其他应用仍可能读取当前系统剪贴板。
- 收件正文与接收游标在同一次加密提交中保存，向服务端确认 `skipped`，不把“存入应用”声称为“已经写入系统剪贴板”。收件区只保留最后一项，按设置的保留天数清理。关闭敏感同步不自动删除已有收件项，可手动删除。
- 普通历史排除敏感正文，使用 AES-256-GCM 独立加密；主密钥由当前用户 DPAPI 保护。历史默认关闭，停用后不再新增，已有记录继续按期限清理。清空历史不会重置同步游标、设备身份或敏感收件项。
- 配对密码通过子进程的匿名标准输入传递，不进入参数、环境变量或配置文件。日常同步使用设备令牌；令牌与待办正文加密保存。
- UI 与后台通过当前登录会话 ACL 限制的本机 Named Pipe 通信，并检查对端进程会话与安装位置。状态不含正文或设备令牌，日志不记录正文，诊断导出仅包含固定状态与版本。
- HTTPS 保护传输，服务端仍可读取正文；当前没有端到端加密。DPAPI 与通道 ACL 不能防御同一用户权限的恶意软件，也不承诺在 SSD 上物理擦除已删除内容。

## 开发与验证

- [验证记录](VALIDATION.md)：原生、配置迁移、剪贴板、IPC、UI 和安装检查及范围限制。
- [设计](DESIGN.md)、[实施计划](DEVELOPMENT-PLAN.md)、[本地通信契约](IPC.md)。
- [后台使用](agent/README.md)。

构建使用 Rust 1.99.1 MSVC、.NET 10、Windows App SDK 2.6.4、Windows SDK 与 Inno Setup 6：先 `cargo build --locked --release --bin rustclipsync-agent`，再执行 `./scripts/build-windows.ps1`。UI 依赖由 NuGet 锁文件固定；共享核心以逐文件 SHA-256 的生成源码快照消费，不手改快照。

当前仓库 [Infinifar/rustclipsync-desktop](https://github.com/Infinifar/rustclipsync-desktop) 为私有；本地目录沿用 `rustclipsync-destop`。服务端和 Android 协议保持兼容，Android 使用 Kotlin 实现同一协议。

相关项目：[服务端](https://github.com/Infinifar/rustclipsync-server) · [HTTP 接口协议](https://github.com/Infinifar/rustclipsync-server/blob/main/PROTOCOL-V1.md) · [Android 输入法](https://github.com/Infinifar/fcitx5-android-bibi-keyboard) · [Omaclipsync](https://github.com/Infinifar/omaclipsync)。
