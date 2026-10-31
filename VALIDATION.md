# Windows 0.2.1 验证

本次增加敏感文本自动写入选项：默认关闭，必须同时启用敏感同步，只影响新接收内容，不补写旧收件项。普通历史仍排除敏感正文，自动写入沿用消息来源、游标和 applying 日志；手动及自动写入均请求 Windows 排除系统历史与云同步。

本次还增加独立敏感收件小窗：默认遮罩、显示/隐藏、时间、置顶、复制和删除；打开时主窗口最小化，重复打开复用小窗。正文按当前 ID 有界读取，状态仍无正文。

本地 Linux 已通过 16 项测试、Win32 类型检查及 Clippy、格式和共享源码哈希校验。原生结果及安装产物待本次 CI 完成后记录。

## 0.2.0 已有验证

日期：2026-10-31。源码开发主机为 Linux x86_64；Windows 构建、实际窗口和截图来自 GitHub Actions 的临时 Windows Server 2025 x64 / VS 2026 虚拟机。未连接或操作 dot（Expert）远程电脑。

## 已通过的检查

| 检查 | 结果与范围 |
| --- | --- |
| Linux `cargo test --locked` | agent 的配置迁移、过滤范围、匿名密码输入、历史加密/清理和 IPC 契约 10 项；共享快照内模块 4 项 |
| Linux Win32 类型检查 | `cargo clippy --locked --features check-windows --all-targets -- -D warnings` 通过，包含原生适配与 Windows 测试 |
| 独立共享核心 | [Core CI 41201673704](https://github.com/Infinifar/rustclipsync-core/actions/runs/41201673704) 的 Windows / Ubuntu 各 22 项测试、格式与 Clippy 均通过 |
| 真实 HTTPS 回归 | 使用临时服务端、独立数据库与临时证书，通过注册、离线最后值、幂等重试、清空/重启不重放、回环抑制、敏感策略与冲突检查 |
| Windows 原生 | DPAPI、私有 ACL、原子替换、排他锁、限长、匿名密码管道、敏感历史排除；另在一次性会话执行真实剪贴板与 IPC 控制测试 |
| 首轮完整 UI | [CI 41202306906](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41202306906) 通过三页导航、主题/字号保存、真实启动项创建/删除、安装、图标、启动和卸载 |
| 后续视觉检查 | 实际截图检查并修复大字号的横向裁切；暗色标题栏、滚动视口宽度与窄窗口布局已检查 |

共享核心新增测试覆盖：私有敏感正文与游标同次加密提交、skipped 确认、保存失败不确认/不写剪贴板、重启不重放、通知身份无正文、按 ID 删除收件项。旧 Omaclipsync 状态迁移测试继续通过。

Windows 原生 IPC 测试启动真实后台，验证当前登录会话通道、脱敏状态、配置更新及版本冲突、过滤规则和退出；确认设备 ID、游标和加密收件项保留，状态不包含令牌或正文。历史测试确认敏感内容不会进入普通加密历史。

## 完整配对后 UI 验证

[最终 CI 41206349593](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41206349593) 在提交 `f8bc96fafea09d831d7226ad3c9d40d353673b91` 上 Windows 与 Ubuntu 作业全部通过。Windows 默认 22 项测试、真实剪贴板 1 项、真实管道 1 项，以及以下实际 UI 检查均通过：

- 从 0.1.0 形式的旧配置、DPAPI 密钥及 RCS1 状态启动，不要求重新配对。
- WinUI 通过真实管道读取“已暂停”与设备状态；UI 自动化树不包含敏感正文。
- 主题和字号在退出/重新打开后保留。
- UI 暂停/恢复实际改变后台配置；保存历史设置由后台验证和原子提交。
- 窗口关闭干净退出，恰好一个 Rust 后台继续存活；随后明确退出后台。
- 宽窗口与 520 像素窄窗口的主要操作保持在窗口边界内，保留截图。
- 安装后 EXE 图标与 Omaclipsync ICO 逐像素相同，卸载移除程序和自启入口，保留用户数据。

安装包与截图：[Windows x64 0.2.0 产物](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41206349593/artifacts/13044905561)。已下载到本机 `dist/windows-0.2.0/`，并核对 SHA256SUMS；安装器 SHA-256 为 `9147a0c31c186154a2ea185a243781abb1c88b98f15206512b9e6b81b64c4298`，大小 67,806,308 字节。

本轮关闭窗口验证记录实际退出码 0，配对界面关闭后恰有一个后台进程继续存活；主题、字号、自启和真实配置写入均通过 UI Automation 验证。

[最终安装复验 41207473083](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41207473083) 使用同一已校验安装包再次通过全部 UI 与卸载测试，也验证了测试工具的临时目录隔离限制。加载稳定后的配对窗口截图已目视检查，标题、图标、敏感遮罩、控件与深色标题栏均正常。

## 设计依据与实际限制

本轮已安装并使用 [OpenAI 官方 winui-app 技能](https://github.com/openai/skills/tree/main/skills/.curated/winui-app)，按其原生导航、控件、主题、响应式布局与实际启动验证指导进行检查；主操作排列遵循用户指定的 Omaclipsync 布局。

采用原生 NavigationView、Expander、ListView、ToggleSwitch、NumberBox、ContentDialog、InfoBar 等控件；主题使用系统资源，无额外 UI 组件库。窗口最小有效宽度为 480，初始尺寸和最小尺寸考虑 DPI，目录与同步逻辑由独立后台承担。

Windows Server CI 不等于所有 Windows 11 日常场景均完成验收。用户已确认 0.1.0 的真实 Windows 安装和推送成功；0.2.0 的日常三端持续同步、RDP/睡眠/多屏 DPI、高对比和 Narrator、异常断电恢复及 24 小时资源测量仍需单独实机验收。本轮不提供未经测量的 Windows 内存或延迟数值，安装包仍未签名。

## 排查过程中已修复的问题

- 管道响应写入后过早断开会丢弃未读缓冲；现在读取单字节传输收据后断开，I/O 有截止时间。
- 页面内容在大字号下按期望宽度扩展，导致右侧裁切；现在绑定实际滚动视口宽度并进行边界检查。
- 配置更新检查过期 revision，历史清理不重置同步身份；历史变更序号让同一秒的连续复制和自动清理也能更新列表。
- 进程退出码验证保留真实进程句柄，避免仅按 PID 查询在退出后丢失 ExitCode。

源码导出、格式、Clippy 与安装检查由 CI 执行，导出快照全部文件有 SHA-256 校验。
