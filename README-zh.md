<p><img src="assets/omaclipsync.svg" alt="RustClipSync 图标" width="64" height="64"></p>

当前版本 **0.5.0**，使用共享核心 **0.4.0** 和协议 **3**（`/api/...`），配套服务端 **3.0.0**，见[发布说明](releases/v0.5.0.md)。旧客户端需继续搭配服务端 2.0.0。

# RustClipSync Desktop

[English](README.md) | 简体中文

轻量的 Windows 11 / Linux 文本剪贴板同步客户端，复用 [rustclipsync-core](https://github.com/Infinifar/rustclipsync-core)。Windows 使用 WinUI 3，Linux 使用 Qt 6/QML；关闭界面后，Rust 后台继续同步。

## 下载与使用

从 [正式发布页](https://github.com/Infinifar/rustclipsync-desktop/releases/latest) 下载对应安装包，发布页附有统一的 `SHA256SUMS`。

| 平台 | 安装包 |
| --- | --- |
| Windows 11 x64 | `RustClipSync-版本-setup-x64.exe`，按用户安装 |
| Debian 12 / Ubuntu 24.04 基线 | 对应发行版的 `.deb` |
| Fedora 43 基线 | `.rpm` |
| Arch / Omarchy | `.pkg.tar.zst` |

安装后打开“设置”，填写 HTTPS 服务端地址、账户和密码即可配对。Linux 安装命令、桌面要求和快捷键配置见 [Linux 使用说明](LINUX.md)。Wayland 需要 data-control 支持；缺少该协议的 GNOME/Wayland 会话暂不支持。

Windows 使用系统共享运行库，缺少依赖时提供微软官方下载入口，见 [Windows 安装说明](WINDOWS.md)。安装包目前未签名，签名进度见 [签名政策](CODE-SIGNING.md)。

## 功能

- **文本同步**：双向、仅发送或仅接收；离线只同步最后一条，持久化游标和消息 ID 防止重复处理。单条文本最大 10 MiB。
- **敏感收件区**：默认遮罩，支持显示、复制、删除及独立置顶小窗；敏感文本自动写入系统剪贴板默认关闭。
- **加密历史**：支持搜索、分页、收藏与清理，默认关闭；开启后默认保留 7 天，可调整期限和条数。敏感正文不进入普通历史。
- **界面资源管理**：Windows 页面首次打开时创建，主窗最小化时暂停轮询、恢复立即刷新；历史按存储实例、修订号和查询条件缓存，无变化时复用列表。
- **桌面集成**：主题、字号、登录自启；Windows 提供托盘和全局快捷键，Linux 可通过桌面快捷键打开历史或敏感小窗。

## 方向上报与协议兼容

本客户端要求协议 **3**（`X-RCS-Protocol: 3`），必须与对应服务端配套升级；旧客户端继续使用旧服务端。版本不匹配时停止同步并提示升级，不会反复提示凭据错误。

连接和重连后，核心先上报单向推送、单向接收或双向方向，收到 ACK 后才接收。开关变更立即上报，WS 控制通道失败时使用认证 HTTP 降级。本地方向和敏感过滤始终保留；上报失败显示“尚未确认”，不会放开接收。暂停保留方向和配对，同时禁止收发；手动收发同样遵守方向和暂停设置。

关闭界面或正常退出后台只表示离线，不等于退出登录。显式重新配对或更换账户前必须确认撤销旧方向会话；失败时提示未确认并保留原身份。方向状态闲置 30 天后由服务端失效。重连补发在最新方向确认后进行，每设备最多补发一条满足接收条件的消息；时间窗口及关闭选项由服务端管理页面决定，客户端不硬编码。详见[方向协议](https://github.com/Infinifar/rustclipsync-server/blob/main/docs/DIRECTION-PROTOCOL.md)。

移动端与 Fcitx 输入法单独发布，不属于本批发布；切换共享服务端前请确认它们的构建支持协议 3。配套 Omarchy 插件为 Omaclipsync **0.4.0**。

## 隐私

“同步已标记的敏感文本”默认关闭。部分应用可能不标记敏感内容，因此不能识别所有敏感信息。

本机状态与历史加密保存，主密钥由 Windows DPAPI 或 Linux Secret Service 保护。密码不写入配置或命令行，日志与普通状态不包含剪贴板正文。HTTPS 保护传输，服务端仍可读取同步内容；当前没有端到端加密。删除收件项只作用于本机，不重置接收游标。

## 开发与发布

[Desktop CI / Release](.github/workflows/desktop.yml) 是统一构建入口，并行验证 Windows 和四种 Linux 发行版。全部检查通过后，将安装包与校验和发布到同一个 GitHub Release。

在主分支更新版本并添加对应的英文发布说明，即可触发正式发布；普通提交只运行 CI，也可在 Actions 手动构建。已发布版本的说明文件更新后会同步到 Release 页面。详见 [发布流程](RELEASING.md)、[验证记录](VALIDATION.md) 与 [后台开发说明](agent/README.md)。

相关项目：[服务端](https://github.com/Infinifar/rustclipsync-server) · [接口协议](https://github.com/Infinifar/rustclipsync-server/blob/main/PROTOCOL.md) · [Android 输入法](https://github.com/Infinifar/fcitx5-android-bibi-keyboard) · [Omaclipsync](https://github.com/Infinifar/omaclipsync)

MIT 许可证；第三方组件见 [THIRD-PARTY.md](THIRD-PARTY.md)。

### 从旧服务端迁移

服务端和客户端必须成套切换。旧实例没有方向撤销接口时，先用其管理员或旧版客户端撤销旧设备令牌，停止后台，然后明确执行：

```bash
rustclipsync-agent forget-pairing --confirm-remote-revocation-unconfirmed
```

这会删除本机设备令牌、待发送项和敏感收件区，保留加密历史和密钥环项；它不代表远端已撤销，旧服务端可能仍授权旧令牌，必须由旧管理员撤销。之后再配对新服务端。协议不支持从不视为撤销成功。

HTTP 降级使用 30 秒在线租约，与管理页活动标记独立。服务端补发窗口为 0 时仅关闭离线存储和补发，符合方向的在线设备仍正常实时收发。
