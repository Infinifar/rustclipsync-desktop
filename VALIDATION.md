# Linux 桌面适配与 Windows 回归验证

日期：2026-10-31。受测代码提交：`efc586b6352c006b95449464e1ac77678dea8cee`。
本轮复用既有共享核心，快照 SHA-256 校验通过；未修改服务端、Android 或 Omaclipsync。

## 构建和安装

[Linux CI 41230626304](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41230626304)
五项作业全部通过，覆盖 Debian 12、Ubuntu 24.04、Fedora 43、Arch x86_64。每个发行版执行 Rust
检查、原生 Qt 构建、隔离剪贴板/密钥环/界面测试、打包、实际安装及安装后的启动检查。
产物为两个分别标注系统基线的 DEB、一个 RPM 和一个 pacman 安装包，附 SHA256SUMS。

另一个非 root 作业在独立无头 Sway 中验证原生 Wayland 剪贴板与 D-Bus 通知。
容器中的桌面测试使用普通用户，独立 HOME、所有 XDG 目录、D-Bus 和临时密钥环；
GNOME Keyring 的 IPC_LOCK 能力只配置在一次性构建容器中。

本地也在临时安装根目录安装了 Arch 包并启动已安装的 Qt 客户端；空数据库测试
跳过了依赖检查，完整发行版容器中的安装检查不跳过依赖。Arch 元数据必须直接位于
归档根目录，打包脚本现在额外调用 pacman 校验。包内文件使用 root:root 所有权。

## 实际验证范围

- 普通 Rust 测试：13 项客户端逻辑、5 项 Linux 存储/IPC/历史测试，以及共享快照
  4 项模块测试；Clippy、格式和共享源码哈希检查通过。
- 隔离原生测试：Secret Service 随机密钥读写与丢失保护、加密状态和历史、真实
  Unix socket、设置版本冲突、按 ID 读取/复制/删除敏感收件项及保留接收游标。
- X11/XFixes 和 Wayland data-control：约 700 KB Unicode 合成文本完整传输、
  敏感标记、默认拒绝自动敏感写入、显式启用/关闭、来源标记和自身事件过滤。
- Qt 6.4（Debian 12）与 Qt 6.12（本机 Arch/Omarchy）：实际 QML 启动和界面到
  Rust 后台集成，主题/字号、配置保存、遮罩/分块预览、独立小窗和置顶标志、
  历史读取、自启入口建立/删除。480 与 760 宽度检查，实际渲染图已目视检查。
- 实际启动第二个界面进程并通过激活 socket 触发历史/敏感小窗开关；最后一个
  快捷键小窗关闭后界面进程正常退出。主窗口走真实关闭路径，之后 Rust 后台
  仍可响应控制请求。激活有同用户检查、限长和收据，覆盖已有缓冲数据的情况。
- 新敏感收件通知使用假桌面通知服务验证真实 D-Bus 投递及突发合并，通知参数
  仅有固定到达提示和应用标识，不含剪贴板正文、账户或设备 ID；启动旧收件项
  不触发通知。

界面集成使用真实 Qt 对象和 Rust 进程，自动化渲染采用 offscreen/software；
窗口置顶标志检查不代表所有窗口管理器都会接受置顶请求。会话控制夹具为暂停状态，
未将测试文本写入用户当前剪贴板，也未连接生产同步服务。

## Windows 功能保留

[Windows CI 41230626313](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41230626313)
继续执行原生 MSVC 构建、DPAPI/ACL、真实剪贴板、管道、全局快捷键、WinUI
界面、居中导航、图标、安装与卸载检查，全部通过。Windows 使用原有 WinUI 前端和原生命名
管道；共用运行逻辑与历史模块通过兼容导出保留原调用路径。

## 当前限制

- Wayland 必须提供所需 data-control 协议；未验收不提供该协议的 GNOME/Wayland
  集成，也不把 XWayland 当作整个 Wayland 桌面的剪贴板。
- Linux 快捷键由桌面绑定 `rustclipsync --history` / `rustclipsync --inbox`，当前
  不提供应用内 GlobalShortcuts Portal 注册。Windows 原有按键注册不受影响。
- 本轮为 x86_64 原生 DEB/RPM/Arch 包；未提供 ARM64、Flatpak 或 AppImage。
  系统 Qt 和 glibc 的最低要求以各包的实际构建基线及依赖元数据为准。
- 本轮未进行不同真实桌面版本的完整矩阵、跨设备长期同步、休眠恢复或新的
  性能/内存基准，不将安装包大小作为常驻内存指标。
- Linux 当前构建产物通过 Actions 提供；既有正式 Windows Release 未被替换。

---

# Windows 0.2.3 验证

[CI 41216584020](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41216584020) 在源码提交 `0c3b8954c3d26cc748f26a422e16a48ab374afc0` 上 Windows / Ubuntu 全部通过。

- Linux 17 项测试；Windows 默认 25 项、真实剪贴板 2 项、原生 IPC 1 项，共 28 项。
- 原生 IPC 验证快捷键更新、真实 RegisterHotKey 冲突、失败保留旧配置与旧绑定、相同偏好重新绑定，以及退出释放组合键。
- 实际 WinUI 验证连接/退出位于敏感收件区下方；新快捷键页保存组合、拒绝重复和不合法组合，配置经 Rust 原子保存。
- 使用真实 SendInput 按键验证：主窗口打开时，两个快捷键分别打开/再次关闭目标窗口；重定向复用原界面进程。
- 主窗口完全退出、只剩一个 Rust 后台时，按键能冷启动历史或敏感小窗，再次触发关闭最后一个窗口并干净退出该界面进程。
- 清空绑定后实际按键不再打开窗口；打开/关闭快捷键窗口不改变系统剪贴板，敏感窗口仍默认遮罩。
- 主页面板与独立历史窗口复用 HistoryView；安装资源包包含两个窗口及该控件的编译 XAML。既有主题、自启、收件区显示隐藏、置顶、复制/删除、安装/卸载验证继续通过。

[安装产物](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41216584020/artifacts/13048696509) 已下载到 `dist/windows-0.2.3/` 并核对 SHA256SUMS。安装器 67,840,096 字节，SHA-256 `84831382845582d2bd37568f81ad45b32781edb292606eae60a4cc603dd2e9f1`。快捷键页、历史窗口和主页截图已检查；当前仍未签名。

验证使用临时 GitHub Windows 会话与隔离配置，没有操作用户的 Windows 实机。特殊系统保留快捷键、其他应用抢占、RDP、多登录会话、异常断电及长期资源测量不在这次验收范围；启动注册冲突会显示错误，不影响独立同步。

## 既有版本验证

# Windows 0.2.2 验证

[CI 41213917516](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41213917516) 在提交 `7b71e4d3e8de9f62092b35430e7c4e9221177987` 上 Windows / Ubuntu 全部通过。

- Windows 默认 24 项测试、真实剪贴板 2 项、原生 IPC 1 项，共 27 项；Linux 16 项测试及 Clippy 通过。
- 主页空收件区始终保留，空状态禁用正文操作但保留设置入口；删除后仍显示空状态。
- 主页明确显示/隐藏、离开页面后重新遮罩，以及四个文本操作同排、顺序正确均由实际 UI Automation 验证；独立小窗原有功能继续通过。
- 主页和小窗复用分块读取，预览最多 64 KiB；复制完整原文、消息 ID 检查与敏感标记未改变。
- 原生剪贴板使用合成的多行 SSH 私钥样式文本验证完整写入、隐私格式及自身事件抑制。
- 独立临时服务端的约 4 KiB / 40 KiB / 530 KiB 文本，带敏感标记和无标记共六组，逐字节上传下载一致。公网通道对普通文本和合成私钥样式文本的无凭据请求均返回 401；未改变生产剪贴板内容。不能据此断言真实客户端失败原因已确定。
- 本机 Omaclipsync 配置允许敏感同步、过滤关闭；其日志近期有上传成功记录，但日志不记录正文，无法确认对应哪次私钥复制。Windows 实机运行状态未直接访问。

[安装产物](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41213917516/artifacts/13047577400) 已下载至 `dist/windows-0.2.2/`；安装器 67,818,842 字节，SHA-256 `3244eeddf96d1b6b54698ac9bf19a4903d549db917dd58cdbba6fb22451eee9b`。主页有内容及空状态截图已检查。安装包仍未签名，SignPath 尚未获批。

## 之前的验证记录

### Windows 0.2.1 验证

本次增加敏感文本自动写入选项：默认关闭，必须同时启用敏感同步，只影响新接收内容，不补写旧收件项。普通历史仍排除敏感正文，自动写入沿用消息来源、游标和 applying 日志；手动及自动写入均请求 Windows 排除系统历史与云同步。

本次还增加独立敏感收件小窗：默认遮罩、显示/隐藏、时间、置顶、复制和删除；打开时主窗口最小化，重复打开复用小窗。正文按当前 ID 有界读取，状态仍无正文。

[最终 CI 41211210740](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41211210740) 在源码提交 `165f7959d519c7438425e46df168928ad09cd232` 上 Windows / Ubuntu 作业全部通过：

- Linux 16 项测试，Win32 类型检查、Clippy、格式与共享源码哈希校验通过。
- Windows 默认 24 项、真实剪贴板 2 项、真实管道 1 项，共 27 项测试通过。
- 原生剪贴板验证默认拒绝自动敏感写入、显式启用与关闭、来源回环抑制；逐项读取验证 Windows 历史、云同步和监控排除格式的值。
- 已有配置升级后保持默认手动收件；自动写入开关可由真实 WinUI 经 IPC 保存，切换不补写旧收件项、不改变设备 ID 或游标。
- 小窗入口在收件区标题旁可见；打开后主窗口实际最小化。默认遮罩、明确显示/隐藏、实际 TOPMOST 置顶/取消置顶、完整原文复制、删除及重复打开复用均通过 UI Automation 与 Win32 检查。
- 正文分块读取检查 Unicode 边界、响应帧限长、旧 ID 与非法偏移；状态不泄露正文或令牌。普通历史的敏感排除继续通过原生测试。
- 小窗删除后收件项为空，接收游标仍为 42；关闭小窗不结束主界面，关闭主界面后恰好一个 Rust 后台继续运行。
- 安装、双击启动、图标、主题/字号持久化、自启、卸载均通过；主窗口、小窗及设置截图已检查。

安装产物：[Windows x64 0.2.1](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41211210740/artifacts/13046069005)。本机已保存到 `dist/windows-0.2.1/` 并核对 SHA256SUMS；安装器大小 67,825,609 字节，SHA-256 为 `c27351426277add3684d3a200d4b23bd7eb70f68ed3798eac56179907c800535`。当前为未签名开发安装包。

敏感接收仍为二选一：私有收件区，或自动写入剪贴板；自动写入不会额外保留新收件项。删除仅作用于本机收件项，不删除服务端内容或系统剪贴板，不重置接收游标。未标记的敏感信息仍不能可靠识别。

排查期间曾出现测试按标题查找 WinUI 窗口的误判；截图和 UI Automation 确认窗口实际存在。现改为按进程、UI Automation 及根 HWND 定位，验证实际最小化和 TOPMOST 标记。[安装复验 41211056484](https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41211056484) 与上述最终完整 CI 均通过。

## 0.2.0 已有验证

日期：2026-10-31。源码开发主机为 Linux x86_64；Windows 构建、实际窗口和截图来自 GitHub Actions 的临时 Windows Server 2025 x64 / VS 2026 虚拟机。未连接或操作 dot（Expert）远程电脑。

### 已通过的检查

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

### 完整配对后 UI 验证

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

### 设计依据与实际限制

本轮已安装并使用 [OpenAI 官方 winui-app 技能](https://github.com/openai/skills/tree/main/skills/.curated/winui-app)，按其原生导航、控件、主题、响应式布局与实际启动验证指导进行检查；主操作排列遵循用户指定的 Omaclipsync 布局。

采用原生 NavigationView、Expander、ListView、ToggleSwitch、NumberBox、ContentDialog、InfoBar 等控件；主题使用系统资源，无额外 UI 组件库。窗口最小有效宽度为 480，初始尺寸和最小尺寸考虑 DPI，目录与同步逻辑由独立后台承担。

Windows Server CI 不等于所有 Windows 11 日常场景均完成验收。用户已确认 0.1.0 的真实 Windows 安装和推送成功；0.2.0 的日常三端持续同步、RDP/睡眠/多屏 DPI、高对比和 Narrator、异常断电恢复及 24 小时资源测量仍需单独实机验收。本轮不提供未经测量的 Windows 内存或延迟数值，安装包仍未签名。

### 排查过程中已修复的问题

- 管道响应写入后过早断开会丢弃未读缓冲；现在读取单字节传输收据后断开，I/O 有截止时间。
- 页面内容在大字号下按期望宽度扩展，导致右侧裁切；现在绑定实际滚动视口宽度并进行边界检查。
- 配置更新检查过期 revision，历史清理不重置同步身份；历史变更序号让同一秒的连续复制和自动清理也能更新列表。
- 进程退出码验证保留真实进程句柄，避免仅按 PID 查询在退出后丢失 ExitCode。

源码导出、格式、Clippy 与安装检查由 CI 执行，导出快照全部文件有 SHA-256 校验。
