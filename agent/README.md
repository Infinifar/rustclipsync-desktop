# Windows Rust 后台

`rustclipsync-agent` 是普通用户会话进程，复用 `rustclipsync-core` 0.2.0。同步、剪贴板、状态/历史加密、游标与本地控制由 Rust 负责；WinUI 窗口关闭后，后台独立运行。

## 功能与数据

- 隐藏 Win32 窗口与 `WM_CLIPBOARDUPDATE`，不上传启动前已有的剪贴板；相同序号的通知不生成新消息。
- `CF_UNICODETEXT` 有界转换，文本同步上限 10 MiB。写入携带消息 ID；在剪贴板锁内复查序号，拒绝覆盖刚发生的本机复制。
- HTTPS 设备令牌、长轮询、断网退避、相同 UUID 重试、离线最后值、持久化游标和 applying 日志。
- 当前用户 DPAPI 保护随机主密钥；AES-256-GCM 保存令牌、待办、游标和私有收件项。普通历史独立加密，排除敏感正文。
- 当前用户文件 DACL、同目录临时文件、flush 与 write-through 原子替换；状态损坏或密钥缺失停同步，不重建身份掩盖错误。
- 跨登录会话排他文件锁；Logon SID / 会话 / 安装位置检查的本机 Named Pipe，详见 [IPC.md](../IPC.md)。

目录为 `%LOCALAPPDATA%\RustClipSync\`：

| 文件 | 内容 |
| --- | --- |
| config.json | 地址、账户、方向、暂停、敏感/过滤/历史策略；无密码和令牌 |
| appearance.json | 主题与字号，不含同步数据 |
| `<identity>.key.dpapi` | DPAPI 保护的主密钥 |
| `<identity>.state` | RCS1 加密状态，包含游标、设备令牌、有界 outbox 和最后一个私有收件项 |
| `<identity>.history` | RCH1 加密普通历史与收藏，密钥由主密钥按用途派生 |
| agent.lock | 排他锁，随句柄释放 |
| diagnostics.json | 用户主动导出的固定状态和版本，无正文、密码、令牌和过滤规则 |

`identity` 由规范化服务端地址与区分大小写的账户名计算。已有 0.1.0 配置与加密状态可直接读取。切换服务器/账户使用不同的加密文件，旧文件不自动删除。自动清理作用于当前配对，后台停止时不会执行清理；卸载保留数据目录。

历史默认关闭，开启后默认普通记录 50 条 / 7 天，可设置条数与期限；收藏最多 50 条，手动删除前保留。单条历史最多 1 MiB，总正文最多 16 MiB。历史错误不回滚已完成的同步。清空历史不改变游标、设备身份或私有收件项。

敏感同步默认关闭。开启后，敏感接收只进入加密收件区，消费游标与正文同次提交，ack 为 skipped；不会自动调用系统剪贴板写入。用户明确复制时才写入，并附带 Windows 历史/云同步排除标记。并非所有应用都提供敏感标记，这些措施不能识别全部敏感信息。

## 构建与运行

Windows 11 x64，Visual Studio C++ 工具和 Windows SDK，Rust 1.99.1 MSVC：

```powershell
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bin rustclipsync-agent
```

日常使用完整 `RustClipSync-0.2.0-setup-x64.exe` 安装包与 WinUI 设置页面。无参数双击后台时打开同目录的 WinUI；单独复制后台文件会显示用途提示。

开发时可独立运行：

```powershell
Start-Process -FilePath .\target\release\rustclipsync-agent.exe -ArgumentList @('pair','https://sync.example.com','your-user') -Wait -NoNewWindow
Start-Process -FilePath .\target\release\rustclipsync-agent.exe -ArgumentList 'run'
```

`pair` 通过控制台隐藏输入密码；WinUI 使用 `--password-stdin` 与匿名子进程管道。密码不允许放在命令行、环境变量或配置里。配对子进程完成后退出，常驻 `run` 使用设备令牌。尽力清理输入缓冲，但不保证库或系统临时副本即时清零。

`autostart` 与 `run` 使用相同后台入口，未配对时安静退出；配置中的暂停状态会保留。自启开关由 WinUI 管理当前用户 Startup 文件夹中的应用快捷方式，不显示 UI。

显式 `pair ... --reset` 需要先退出后台，且会放弃当前待发送内容和私有收件项。使用同一服务器和账户重配时保留同步设置。不会自动撤销旧设备记录，可在服务端管理页面删除。

## 配置与控制

建议通过 UI / Named Pipe 修改配置。运行时直接编辑 JSON 不会热加载；手工编辑应先退出后台。

```json
{
  "server":"https://sync.example.com/",
  "user":"your-user",
  "direction":"both",
  "paused":false,
  "allowSensitive":false,
  "intervalSec":60,
  "filter":{"mode":"off","patterns":[],"downloads":false},
  "history":{"enabled":false,"limit":50,"days":7}
}
```

方向为 both/upload/download；过滤为 off/blacklist/whitelist，最多 32 条正则、每条 512 字节。备用检查间隔为 30–300 秒，正常连接使用实时元数据通知。未知字段、非法范围、无效正则或更改配对身份的更新被拒绝。

`rpc` 开发入口从标准输入读一个请求，通过安全管道返回 JSON。协议错误用固定错误码，不输出底层正文/凭据错误：

```powershell
'{"id":"inspect","ipcVersion":1,"method":"getStatus","params":{}}' | .\target\release\rustclipsync-agent.exe rpc | ConvertFrom-Json
```

冲突时手动发送是明确选择当前本机内容；接收会尝试放弃未发送项后读取未消费的远端消息。仍有请求结果不确定时不会静默覆盖。存储错误停自动副作用，需先检查数据与权限。DPAPI 不防同用户恶意程序，磁盘加密与 HTTPS 不等于端到端加密。

## 验证

Linux 上运行逻辑测试与类型检查：

```bash
cargo test --locked
cargo clippy --locked --features check-windows --all-targets -- -D warnings
python3 scripts/verify-core.py
```

Windows 默认执行 DPAPI、原子替换、ACL、锁、文件限长、匿名密码管道与敏感历史排除测试。以下测试涉及当前会话剪贴板 / 托盘 / 管道，只能在一次性 VM 中显式执行：

```powershell
$env:RCS_WINDOWS_DISPOSABLE='1'
cargo test --locked --test windows_clipboard -- --ignored --test-threads=1
cargo test --locked --test windows_control -- --ignored --test-threads=1
```

CI 另外执行实际安装、WinUI 三页、主题/字号持久化、真实自启项、配对后的 C# → Rust 控制与 UI 退出后后台存活检查。结果与待验收范围见 [VALIDATION.md](../VALIDATION.md)。

共享源码快照由 `scripts/sync-core.sh` 从独立项目导出。不要手工编辑 vendor；校验器在独立检出中验证全部导出文件的 SHA-256。
