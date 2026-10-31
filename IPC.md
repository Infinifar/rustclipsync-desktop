# WinUI 与 Rust 的本地通信草案

此文档定义后续实现边界，尚无 Named Pipe 代码。它不是服务端 HTTP 协议的新版本。

## 通道与权限

使用本机双向 Named Pipe，名称按产品、用户配置和登录会话区分。Rust 创建显式安全描述符，限制到当前登录会话的 Logon SID；设置拒绝远程客户端。不要使用默认 DACL，因为默认权限可能允许 Everyone/匿名读取。[微软 Named Pipe 安全文档](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights)

首次创建使用防止已有服务端实例抢占的选项；UI 和 agent 校验对端进程用户、会话及安装位置。Pipe 名、客户端自报 PID、用户名都不是身份认证。该设计不承诺防御同一用户权限的恶意软件。

## 帧与生命周期

- 字节流：4 字节小端无符号长度，然后是 UTF-8 JSON。
- 整帧最大 1 MiB，命令和响应 DTO 有更小的字段上限。正文按分页/有界预览显示，不在状态事件中携带 10 MiB 文本。
- 单连接限制未完成请求数；连接与帧读取有超时。事件采用有界队列，状态可合并；落后客户端重新获取快照。
- 协商 `ipcVersion: 1`，未知不兼容版本拒绝连接。该数字不出现在产品名称里。
- UI 连接时发送 hello、读取快照并订阅事件。UI 关闭断开订阅，不暂停 agent；全局退出必须发明确命令。

## 消息形状

```json
{"id":"req-42","method":"getStatus","params":{}}
```

```json
{"id":"req-42","ok":true,"result":{"state":"connected","paused":false,"pending":false}}
```

```json
{"id":"req-42","ok":false,"error":{"code":"storage_error","retryable":false}}
```

```json
{"event":"statusChanged","sequence":18,"data":{"state":"offline","pending":true}}
```

事件 sequence 仅用于此 agent 生命周期的 UI 更新，不作为消息 revision。异常只返回固定错误码和允许公开的字段；不得透传包含凭据/正文的底层错误字符串。

## 命令范围

| 命令 | 行为 |
| --- | --- |
| hello / getStatus / subscribe | 协商、快照、增量事件 |
| getSettings / updateSettings | 读取脱敏配置、验证并保存配置；拒绝未知字段 |
| pair / rePair | 受保护通道内提交密码；rePair 要求明确确认丢弃旧身份/待发送内容 |
| setPaused / setDirection | 设置目标状态，不用可重复翻转的 toggle 命令 |
| pushNow / pullNow | 明确的手动操作，仍遵守敏感策略；冲突覆盖须带确认 |
| listHistory / listInbox | 分页返回受限预览，默认遮挡敏感项 |
| copyItem | 通过条目 ID 让 agent 写入剪贴板；正文不必经过 UI |
| deleteHistory / clearHistory | 删除历史，不修改 cursor/device identity |
| exportDiagnostics | 仅导出脱敏诊断 |
| quit | 保存状态并退出 agent |

普通配置响应不包含设备 token、状态密钥、账户密码。`pair` 密码不写配置文件，不进入日志；完成后释放输入缓冲区，避免承诺托管内存一定即时清零。

请求 ID 在连接内唯一。读操作允许重试；非幂等动作断线后先查状态，不自动再次执行。后续实现须为手动上传等操作增加可查询的操作 ID，或在会话内缓存结果，防止 UI 超时重试生成另一条复制消息。

契约文件与 C#/Rust DTO 必须配套跨语言样例测试，枚举、可空字段、整数范围、最大长度及错误码统一维护。UI 展示消息与 Rust 同步状态分离，不能把 UI 内部页面状态写入核心协议对象。
