# WinUI 与 Rust 本地通信

0.2.0 已实现请求/响应 Named Pipe。当前 UI 在打开时每 2 秒读取快照，关闭窗口停止查询；没有常驻 .NET 同步循环。后续事件订阅不属于本版实现。服务端 HTTP 协议不变。

## 通道与边界

管道名为 `RustClipSync.<Windows 用户 SID 的 SHA-256 前 16 位>.<SessionId>`。用户与会话区分不构成身份认证，权限由以下措施实施：

- 使用显式、受保护的 DACL，仅授予当前 Logon SID 读写权限。
- 设置 `PIPE_REJECT_REMOTE_CLIENTS`、`FILE_FLAG_FIRST_PIPE_INSTANCE`。
- 双方检查管道对端 PID、登录会话与安装目录的实际映像路径。后台仅接受同目录 WinUI/agent 组件。
- 客户端不允许后台通过通道冒用自己的令牌；同用户恶意软件不在该隔离边界的防御范围内。
- Win32 I/O 使用 overlapped 事件等待、截止时间和取消，空闲时不做忙等。

## 帧与交付

一次连接处理一次请求：

1. 客户端发送 4 字节小端长度与 UTF-8 JSON。
2. 后台返回相同结构的响应。
3. 客户端读完整个响应后，发送单字节 `0x01` 作为传输收据。
4. 后台读到收据后才断开。否则 `DisconnectNamedPipe` 会丢弃尚未读完的响应缓冲。

请求与响应各不超过 256 KiB。帧读写限时 4 秒，命令队列最多 16 个请求，执行前检查截止时间；配置/网络操作最长等待确认 60 秒，UI 连接及整次操作也有超时。管道关闭后不会自动重发操作。退出命令在响应收据之后才触发后台关闭。

传输收据不等于服务端剪贴板消息的 ack；消息游标及 applied/skipped 确认仍由共享核心管理。

```json
{"id":"a-unique-request-id","ipcVersion":1,"method":"setPaused","params":{"paused":true}}
```

```json
{"id":"a-unique-request-id","ok":true,"result":{}}
```

```json
{"id":"a-unique-request-id","ok":false,"error":"settings_changed"}
```

`ipcVersion` 仅用于兼容性，产品页面不使用该数字。ID 为连接请求的关联标识；UI 不自动重试非幂等操作。出现 `operation_unconfirmed` 时先刷新状态，不能把未收到响应当作操作未执行。

## 命令

| 方法 | 参数 / 行为 |
| --- | --- |
| getStatus | 状态、暂停标记、脱敏配置、配置 revision、设备 ID、游标、待办、最近同步、历史错误/变更序号与敏感收件项元数据；无正文和令牌 |
| setPaused | `{paused: bool}`，设置目标状态并持久化 |
| updateSettings | `{config: ..., expectedRevision: u64}`，验证并原子保存；拒绝变更配对身份与过期 revision |
| pushNow / pullNow | 用户明确触发；保持敏感/过滤/游标规则，冲突覆盖在 UI 请求确认 |
| testFilter | `{filter: ..., text: ...}`，最多 64 KiB 测试文本，不写剪贴板或服务端 |
| listHistory | `{offset, query, favorites}`，每页 20 条、每条最多 240 字符预览 |
| copyHistory / deleteHistory | `{id}`，后台按 ID 查找；复制正文无需通过 UI |
| setFavorite | `{id, favorite}`，最多 50 条并受总容量约束 |
| clearHistory | 清理普通历史与收藏，不修改同步身份、游标或敏感收件项 |
| copyInbox / clearInbox | `{id}`，验证仍是当前收件项，防止陈旧命令作用到后来的消息 |
| exportDiagnostics | 固定状态、版本、暂停/待办/历史错误，不含账号、正文、令牌或过滤规则 |
| quit | 完成响应交付后保存/退出后台 |

配置新增可选布尔字段 `autoApplySensitive`（默认 false）。只有与 `allowSensitive` 同时为 true 时才自动写入之后收到的敏感文本；设置更新不补写旧收件项，不重置游标或设备身份。旧配置缺少该字段时保持手动收件行为。它随 `getStatus.config` 返回，经 `updateSettings` 原子保存并立即应用。

首次配对仍使用专用 `pair --password-stdin` 子进程。账户密码不进入通用状态通道，不落盘；重新配对必须经 UI 明确确认。主题/字号为独立的非敏感界面偏好；自启设置直接管理经过目标验证的当前用户 `.lnk`，与同步配置分开。

固定错误码包括 `invalid_request`、`invalid_filter`、`busy`、`settings_changed`、`storage_error`、`history_error`、`credentials`、`conflict`、`expired`、`re_pair`、`clipboard_error`、`operation_unconfirmed` 和 `needs_restart`。不透传底层 HTTP/正文错误字符串。
