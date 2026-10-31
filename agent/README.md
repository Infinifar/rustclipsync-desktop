# Windows Rust 后台

`rustclipsync-agent` 是普通用户会话进程，复用 `rustclipsync-core`。当前可独立配对、运行同步并通过系统托盘暂停/退出，WinUI 与 Named Pipe 尚未接入。

## 已实现

- 隐藏 Win32 窗口监听 `WM_CLIPBOARDUPDATE`；本地事件合并为当前最后一条，不上传启动前已有的剪贴板。
- `CF_UNICODETEXT` 有界 UTF-16/UTF-8 转换，正文最大 10 MiB，不创建整份中间文本副本。相同剪贴板序号的重复通知不创建新消息。
- 写入携带私有消息 ID，抑制本机回声；取得剪贴板锁后再次检查序号，拒绝覆盖新复制。占用时单次重试最多约 200 ms，读失败后继续等待/重试。
- HTTPS 校验证书、设备 token、长轮询和断网退避；重试 UUID、离线最后值、游标和应用日志由共享核心处理。
- DPAPI CurrentUser 保护随机状态密钥；AES-256-GCM 加密设备身份与待发送文本；目录/文件使用当前用户专用的受保护 DACL。
- 同目录随机临时文件、内容 flush 与 write-through 替换；状态损坏、密钥缺失及存储失败停同步，不静默建立新设备身份。
- 持有跨登录会话有效的排他文件句柄，防止两个 agent 写同一配置；退出时停止新操作、保留持久状态并等待当前同步批次返回。

## Windows 构建

使用 Visual Studio 的 **Desktop development with C++** 工具和 Windows SDK，以及 Rust MSVC。CI 固定 Rust 1.99.1，开发目标为 Windows 11 x64：

```powershell
rustup toolchain install 1.99.1 --profile minimal --component clippy,rustfmt
rustup override set 1.99.1
cargo test --locked -- --test-threads=1
cargo clippy --locked --all-targets -- -D warnings
cargo build --locked --release --bin rustclipsync-agent
```

产物是 `target\release\rustclipsync-agent.exe`，开发构建尚未签名。普通运行使用 Windows GUI 子系统，不显示常驻控制台。配对命令连接父控制台；PowerShell 用 `Start-Process -Wait -NoNewWindow` 等待其完成：

```powershell
Start-Process -FilePath .\target\release\rustclipsync-agent.exe -ArgumentList @('pair','https://sync.example.com','your-user') -Wait -NoNewWindow
Start-Process -FilePath .\target\release\rustclipsync-agent.exe -ArgumentList 'run'
```

配对通过控制台隐藏输入密码，不支持密码命令行参数或明文配置。配对结束后该进程退出；日常 `run` 使用设备 token，不保留注册时的账户密码。输入缓冲尽力清理，但不承诺所有库/系统内存中的临时副本即时清零。

系统托盘右键可暂停、恢复、退出。悬停显示 `connected`、`offline`、`paused`、`conflict`、`re_pair`、`credentials` 或 `storage_error` 等固定状态。暂停期间不补传复制内容；恢复后监听新的复制。网络中断期间仍保留最后一条本地待发送内容。

已有配置需要重配时，先从托盘退出，再显式使用 `pair ... --reset`。该操作会重建设备身份并放弃当前身份的待发送内容；不会自动撤销服务端的旧设备，需在服务端管理界面删除。配置其他服务器/账户时使用不同的本地加密状态文件，旧文件不会自动删除。

## 数据与配置

目录：`%LOCALAPPDATA%\RustClipSync\`。

| 文件 | 内容 |
| --- | --- |
| `config.json` | HTTPS 地址、账户名、方向与暂停状态，无密码/token |
| `<identity>.key.dpapi` | 当前用户 DPAPI 保护的状态加密密钥 |
| `<identity>.state` | 核心加密状态，含设备 token、游标与有界 outbox |
| `agent.lock` | 排他锁文件，退出后文件可保留，锁随句柄释放 |

配置样例（方向可为 `both`、`upload`、`download`；修改后重启 agent）：

```json
{"server":"https://sync.example.com/","user":"your-user","direction":"both","paused":false}
```

敏感同步与长期历史在当前阶段均保持关闭，不接受密码等未知配置字段。识别 `ExcludeClipboardContentFromMonitorProcessing`，以及明确为零的 `CanIncludeInClipboardHistory` / `CanUploadToCloudClipboard` 提示作为敏感信号。这些提示不是完整敏感检测，应用可能不标记。收到已标记敏感消息会 skipped，不自动写入系统剪贴板。

系统剪贴板写入结果不确定时，恢复日志会消费为 skipped，优先防止重放，可能漏收一次。冲突、过期、设备失效、存储错误会停止自动同步；当前阶段通过退出、排障后重启/显式重配恢复，后续由 WinUI 提供完整冲突处理。DPAPI 不防同一用户权限的恶意程序，HTTPS 与磁盘加密不等于端到端加密。

## 测试边界

```bash
# Linux 上可执行的逻辑测试
cargo test --locked
# 仅类型检查 Win32 模块及原生测试，不在 Linux 执行 Windows API
cargo clippy --locked --features check-windows --all-targets -- -D warnings
python3 scripts/verify-core.py
```

Windows 默认测试额外执行 DPAPI、状态替换、排他锁、密钥缺失、DACL 与文件限长验证，均使用临时目录。真实剪贴板测试默认忽略，只有在一次性的 Windows VM/测试会话中执行：

```powershell
$env:RCS_WINDOWS_DISPOSABLE='1'
cargo test --locked --test windows_clipboard -- --ignored --test-threads=1
```

这项测试会覆盖测试会话剪贴板，不应在日常桌面运行。Windows CI 会在一次性的 GitHub 托管 runner 中显式执行它；日常电脑仍默认忽略。当前本机验证结果及待验收项见 [VALIDATION.md](../VALIDATION.md)。

共享核心源码快照由 `bash scripts/sync-core.sh` 从相邻项目导出；不要直接编辑 `agent/vendor/rustclipsync-core`。`verify-core.py` 可在独立检出中校验全部导出文件的哈希。
