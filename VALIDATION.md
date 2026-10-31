# Rust 后台阶段验证

日期：2026-10-31。开发主机为 Linux x86_64；系统 Rust/Cargo 1.99.1。

## 本机已通过

| 检查 | 结果 |
| --- | --- |
| `cargo test --locked` | 8 项通过：agent 的 URL/凭据字段检查与 UTF-16/UTF-8 限长 4 项，共享快照内模块测试 4 项 |
| `cargo clippy --locked --features check-windows --all-targets -- -D warnings` | 通过；包括全部 Win32 适配代码与原生测试的类型检查 |
| `cargo fmt --all -- --check` | 通过 |
| 独立共享核心快照校验 | 所有文件与 UPSTREAM.json 的 SHA-256 一致，未修改消费端快照 |
| 本地项目依赖 | 仅使用仓库内导出源码和锁定的 registry 依赖，可独立检出构建 |

`check-windows` 在 Linux 上只用于编译检查，不执行 Windows API，不生成 Windows 可运行文件。共享核心之前完成的真实 HTTPS/状态迁移测试不计入本阶段新增 Windows 实机结果。

## 已准备、尚未执行的 Windows 验收

`.github/workflows/windows.yml` 固定 Rust 1.99.1，在 Windows x64 MSVC 上执行默认测试、Clippy 和 release 构建，生成未签名的开发 artifact。仓库尚未推送并触发该工作流，不能称其已经通过。

Windows 默认额外执行 6 项原生测试：DPAPI 加解密及篡改、加密状态原子替换、已有锁文件的互斥、密钥缺失不重建、目录/文件保护 DACL、私有文件大小限制。

另有 1 项默认忽略的剪贴板测试，验证来源标记、重复序号通知及写入前的序号冲突拒绝。只允许在设置 `RCS_WINDOWS_DISPOSABLE=1` 的临时 Windows 会话中执行，会覆盖该会话剪贴板。

本机曾用隔离目录中的官方 Rust MSVC 目标库、编译器及 Windows SDK UCRT 头尝试交叉检查；原生 C 依赖 `ring` 仍需要完整 Visual C++ CRT 头文件（缺少 `vcruntime.h`）。这不计作 Windows 目标构建通过，也没有通过替换加密实现或伪造头文件绕过。系统工具链未修改。

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

当前交付范围是 Rust 后台及 Windows 适配源码。WinUI、Named Pipe、私有敏感收件箱、长期历史、开机启动和 MSIX 打包仍按开发计划推进。

## 实现依据

- [SetClipboardData：内存所有权与有效 owner 窗口](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-setclipboarddata)
- [AddClipboardFormatListener：剪贴板更新通知](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-addclipboardformatlistener)
- [MoveFileExW：同目录替换和 write-through 标志](https://learn.microsoft.com/en-us/windows/win32/api/winbase/nf-winbase-movefileexw)
- [CryptProtectData：当前用户 DPAPI](https://learn.microsoft.com/en-us/windows/win32/api/dpapi/nf-dpapi-cryptprotectdata)

以上 API 文档不构成对本应用运行正确性或断电行为的实机验证。
