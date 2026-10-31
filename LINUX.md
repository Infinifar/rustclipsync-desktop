# Linux 客户端

Linux 使用 Qt 6/QML 独立界面和 Rust 常驻后台，无需 Omarchy、Quickshell 或
浏览器运行时。与 Windows 共用桌面后台的配置、命令处理、加密历史，以及
现有 [rustclipsync-core](https://github.com/Infinifar/rustclipsync-core) 同步状态机。
服务端和 Android 协议不变，不支持旧 SyncClipboard 协议。

## 支持范围

首批构建为 x86_64：Debian 12、Ubuntu 24.04、Fedora 43、Arch。
工作流为每个发行版单独编译、测试和打包，版本和依赖基线标注在产物名称中。
包构建成功不代表已测试该发行版的所有桌面环境。

| 桌面会话 | 剪贴板适配 |
| --- | --- |
| 支持 wlr/ext data-control 的 Wayland 合成器，如相应版本的 Hyprland、Sway、KDE | 原生事件监听，必须实际提供所需协议 |
| 原生 X11 | XFixes 监听普通 CLIPBOARD，支持有界 INCR 大文本传输 |
| 不提供 data-control 的 GNOME/Wayland 等会话 | 暂不能后台同步；显示启动错误，需要后续专用集成 |
| 无图形会话 | 后台不启动；本程序不是系统级剪贴板服务 |

Wayland 会话不会回退为 XWayland 并宣称可以监控整个桌面。只同步普通文本，
不读取主选择区；图片、文件和富文本不在当前范围。单条文本最大 10 MiB。

## 安装与启动

在 GitHub Actions 的 **Linux desktop** 构建产物中下载匹配发行版的包。
首次使用需启动并解锁支持 Secret Service 的桌面密钥环，例如 GNOME Keyring
或启用了对应接口的 KWallet。没有密钥环时不会回退为明文密钥文件。

```bash
# Debian / Ubuntu：选择与系统基线匹配的 .deb，apt 同时处理依赖
sudo apt install ./rustclipsync-desktop-*-debian12-x86_64.deb

# Fedora
sudo dnf install ./rustclipsync-desktop-*-fedora43-x86_64.rpm

# Arch / Omarchy：与现有 Omaclipsync 插件分别安装，不自动迁移配置
sudo pacman -U ./rustclipsync-desktop-*-arch-x86_64.pkg.tar.zst

rustclipsync
```

主页保留发送、接收、暂停、方向选择、默认展示的敏感收件区和普通历史。
外观支持系统/浅色/深色主题和字号。设置页提供 HTTPS 服务端配对、敏感策略、
过滤规则、历史清理期限及登录自启。

关闭界面后，Rust 后台继续同步；“退出后台”会停止同步并关闭界面。
再次打开应用可重新启动后台。登录自启只启动后台，不自动弹出界面。
本次安装不会启动或修改现有 Omaclipsync 插件；同时开启两个客户端可能
造成额外复制事件，请按实际需要选择一个常驻客户端。

## 敏感内容与本机隐私

- “同步已标记的敏感文本”默认关闭；应用未标记的内容无法可靠识别。
- 敏感内容默认进入加密收件区，只保留最新一条，不进入普通历史。
- 自动写入系统剪贴板默认关闭，需同时启用敏感同步；仅作用于新消息，
  不补写旧项。自动写入与保存新收件项目前为二选一。
- 小窗默认遮罩，支持显示、置顶、复制和删除。失去焦点、切换页面或关闭
  窗口会重新隐藏预览。预览最多 64 KiB，复制按钮复制完整原文。
- 手动复制保留敏感与来源标记，抑制自身复制回环。其他剪贴板管理器是否
  尊重敏感标记由其实现决定。
- 删除仅移除本机收件项，不重置接收游标，不清空系统剪贴板或服务端内容。
- 普通历史默认关闭；启用后加密保存。默认保留 7 天，后台定时清理当前身份
  的过期普通历史与敏感收件项；普通收藏保留到手动删除。
- 使用 HTTPS 并验证证书。这是传输与本机静态加密，服务端仍能读取同步内容。

状态与历史目录为 `$XDG_STATE_HOME/rustclipsync-desktop`，未设置时使用
`~/.local/state/rustclipsync-desktop`。目录权限 0700，数据文件权限 0600，
状态和普通历史采用既有加密格式。32 字节随机密钥保存在 Secret Service，
按账户身份和配置目录隔离；密钥丢失/损坏时不会静默重建现有身份。

界面通过 `$XDG_RUNTIME_DIR/rustclipsync-desktop/` 中的私有 Unix socket
连接后台，使用 Windows 相同的 JSON 命令格式，不监听 TCP。双方验证本机
用户身份；状态、诊断和通知不携带剪贴板正文、密码或令牌。

## 快捷键

在桌面环境的快捷键设置中绑定：

```bash
rustclipsync --history
rustclipsync --inbox
```

再次执行会关闭对应小窗。命令通过单实例激活现有界面；仅由快捷键启动界面
时，关闭最后一个小窗会结束 Qt 进程，Rust 后台继续运行。
Linux 当前由桌面环境管理按键组合，暂不提供 Windows 式应用内全局按键注册，
不把 Windows 虚拟键配置直接套到 Wayland。

## 从源码构建

需要 Rust 1.99.1、C++17 编译器、CMake 3.22+、Ninja、pkg-config、libdbus 开发包、
Qt 6.4+ 的 Core/Gui/Network/Quick/QuickControls2 开发包和对应 QML 模块。
Wayland 界面还需 Qt Wayland 平台插件。

```bash
python3 scripts/verify-core.py
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
bash scripts/build-linux.sh --stage

# 以当前发行版原生依赖打包，不能跨发行版更换标签冒充兼容
python3 scripts/package-linux.py deb --distro debian12
python3 scripts/package-linux.py rpm --distro fedora43
python3 scripts/package-linux.py arch --distro arch
```

运行产物位于 `build/linux/rustclipsync`；完整暂存目录为 `dist/linux-root/usr/`，
可从其 `bin/rustclipsync` 启动界面及相邻 libexec 后台。
DEB 构建额外需要 dpkg-dev，RPM 需要 rpmbuild，Arch 需要 bsdtar 和 zstd。
工具可生成带发行版基线名称的 tar.gz，仍依赖系统 Qt，不能视为通用 AppImage。
Flatpak、AppImage、ARM64 不在本轮发布范围。

## 验证

普通 Rust 测试无需触碰真实剪贴板。原生验证使用以下脚本创建隔离显示服务器、
独立 D-Bus、临时 HOME/XDG 目录和一次性密钥环：

```bash
# 安装 Xvfb、dbus-run-session、gnome-keyring-daemon 后运行
python3 scripts/test-linux-native.py
```

若已经构建暂存安装目录，还会运行实际 Qt 界面到 Rust 后台的集成检查。
本机存在 Wayland 会话而没有 Xvfb 时，可以用独立 Xwayland 服务器验证 X11
适配；不会操作当前桌面的剪贴板。

Windows 原有原生剪贴板、DPAPI、IPC、快捷键、WinUI 和安装/卸载验证保持在
`.github/workflows/windows.yml`；Linux 各发行版独立执行构建和测试。
完整记录见 [VALIDATION.md](VALIDATION.md)。
