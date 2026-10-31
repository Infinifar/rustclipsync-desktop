# Windows 客户端验证

日期：2026-10-31。开发主机为 Linux x86_64；系统 Rust/Cargo 1.99.1。

## 本机已通过

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked` | 10 项通过：agent 的 URL/凭据字段、UTF-16/UTF-8 限长和匿名管道密码输入 6 项，共享快照内模块测试 4 项 |
| `cargo clippy --locked --features check-windows --all-targets -- -D warnings` | 通过；包括全部 Win32 适配代码与原生测试的类型检查 |
| `cargo fmt --all -- --check` | 通过 |
| 独立共享核心快照校验 | 所有文件与 UPSTREAM.json 的 SHA-256 一致，未修改消费端快照 |
| 本地项目依赖 | 仅使用仓库内导出源码和锁定的 registry 依赖，可独立检出构建 |

`check-windows` 在 Linux 上只用于编译检查，不执行 Windows API，不生成 Windows 可运行文件。共享核心之前完成的真实 HTTPS/状态迁移测试不计入本阶段新增 Windows 实机结果。

## GitHub Windows 原生验证已通过

[Windows 客户端 CI 41191068330](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41191068330) 在提交 `f9f35a74dcf2458a55af3f4178a4a8a9a874ce24` 上全部通过，使用 GitHub 临时 Windows Server 2025 x64 / VS 2026 runner 与 Rust 1.99.1 MSVC；Ubuntu 作业同时通过。

- Windows 默认测试 14 项通过：agent 逻辑 4 项、DPAPI/存储/权限/锁 6 项、内嵌共享模块 4 项。
- 真实剪贴板测试在 `RCS_WINDOWS_DISPOSABLE=1` 的临时 runner 显式执行，1 项通过：来源标记、自身写入和重复通知过滤，以及拒绝过期序号覆盖。
- 格式检查、Clippy、共享源码哈希检查、MSVC release 构建均通过。
- 构建产物 `rustclipsync-agent-windows-x64-development` 已上传为 [artifact 13038334662](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41191068330/artifacts/13038334662)，是未签名的开发构建。
- 二进制已下载到本地 `dist/windows-x64/rustclipsync-agent.exe`，同目录保存 SHA256SUMS。

本轮原生验证修复了两处平台差异：源码快照校验统一使用 POSIX 相对路径，Git 强制文本 LF；自身写入确认改为在剪贴板锁内核对 owner HWND 和唯一消息标记，再记下最终观察到的序号，避免关闭剪贴板时序号变化导致误判。

共享核心本身另通过 [Windows / Ubuntu 的 20 项完整测试及 Clippy](https://github.com/Infinifar/rustclipsync-core/actions/runs/41190778425)，不是只运行消费端快照内的 4 项模块测试。

早期本机交叉检查曾因缺少完整 Visual C++ CRT 头而受限；本次已由 GitHub 的真实 Windows MSVC 环境完成原生编译、测试和 release 产物生成。Windows Server runner 的测试不等于下面所有 Windows 11 日常桌面场景都已验收。

## 实机检查清单

- 在 Windows 11 x64 临时用户/VM 中执行默认测试与专用剪贴板测试。
- 配对临时服务端，确认密码不写盘；退出后以空账户密码的日常 Client 恢复 token 同步。
- 确认启动不上传旧剪贴板，A→B→A 正常、远端写入不回环，清空/重启不重放，离线复制只同步最后一条。
- 让其他进程占用剪贴板，验证有限重试、暂时读错误恢复，以及下载期间本地复制不会被覆盖。
- 验证标记敏感内容默认过滤，空/非文本/非法 UTF-16、10 MiB 文本与超限内容的行为。
- 测试暂停/恢复、Explorer 重启、睡眠/恢复、RDP 同会话重连、同用户另一会话争用锁，以及托盘退出。
- 测试磁盘满、权限错误、状态篡改、断电/强杀，确认停同步且没有新建身份掩盖错误。
- 测试网络响应丢失、409、410、设备撤销；当前冲突/存储错误通过固定状态提示并停止自动同步。
- 最后进行资源、延迟与持续运行测量。本轮没有 Windows 性能数值或签名安装包。

当前已增加 WinUI 首次配置窗口与按用户的 EXE 安装器，包含开始菜单、可选桌面快捷方式和可选登录启动。完整 Named Pipe、私有敏感收件箱、长期历史、四页设置和 MSIX 仍按开发计划推进。

## 实现依据

- [SetClipboardData：内存所有权与有效 owner 窗口](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata)
- [AddClipboardFormatListener：剪贴板更新通知](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-addclipboardformatlistener)
- [MoveFileExW：同目录替换和 write-through 标志](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [CryptProtectData：当前用户 DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)

以上 API 文档不构成对本应用运行正确性或断电行为的实机验证。

## 首次安装与图标修复

旧版产物只有 Rust 后台 EXE；无参数时向隐藏的标准输出打印帮助后退出，没有安装向导或设置窗口，也没有嵌入应用图标。这解释了双击无反应和没有图标的现象。

本次加入 WinUI 3 / .NET 10 首次配置窗口和 Inno Setup 按用户安装器，图标直接复用 Omaclipsync 的 SVG 并导出 PNG/ICO。后台无参数启动时转到相邻的 WinUI 程序；缺少界面组件时显示明确提示。安装包包含 .NET、Windows App SDK 2.6.4 和应用本地 C++ 运行组件。

Windows 原生测试已扩展到默认 17 项与专用剪贴板 1 项；新增 GUI 子进程匿名标准输入测试，覆盖真实 Win32 无控制台密码传输、Unicode 和失败时不保存配置。嵌入图标检查比较实际交付 EXE 与原始 ICO 的像素。

早期安装验证实际发现并阻止交付了资源加载失败的构建：WinUI 编译通过，但发布目录缺少应用 XAML 资源。SDK 在启用 WinUI 构建工具后已生成包含应用与控件主题的 `RustClipSync.pri`；构建脚本将该完整索引以未打包运行时需要的 `resources.pri` 名称发布，并检查 MainWindow 与主题 XBF 条目。

[安装复验 41197073934](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41197073934) 已通过按用户安装、实际 EXE 图标像素比较、无参数双击打开原生 WinUI 窗口、截图与卸载。复验使用提交 `6f27033` 构建的安装包、提交 `8d830d2` 的校验脚本。实际截图已下载并目视检查：中文标签、同款图标、原生输入框与按钮均正常，没有启动错误。

测试脚本还修正了 Windows 路径字符串混用斜杠，导致有窗口时仍无法识别进程的问题。

[完整 Windows/Ubuntu CI 41197072246](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41197072246) 已在提交 `8d830d209f0aa24863e59fb62635c82d8e66fc47` 上全部通过：Windows 默认 17 项与真实剪贴板 1 项、格式/Clippy/共享源码校验、Rust release、WinUI 自带运行组件构建、安装/图标/窗口/卸载均成功；Ubuntu 的 10 项测试和 Clippy 同时通过。

最终安装包、SHA256SUMS 与实际首次运行截图见 [Windows x64 安装产物](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41197072246/artifacts/13041411062)。已下载到本地 `dist/installer/`，安装器为 `RustClipSync-0.1.0-setup-x64.exe`。这是未签名的开发安装包。

Windows 11 日常双向同步、完整托盘交互、升级、长时间运行和代码签名仍需单独验收。

资源发布依据：[微软未打包应用资源流程](https://learn.microsoft.com/en-us/windows/apps/windows-app-sdk/mrtcore/localize-strings#loading-strings-in-unpackaged-applications)。
