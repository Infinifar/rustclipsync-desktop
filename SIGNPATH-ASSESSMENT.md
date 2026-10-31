# SignPath 申请评估

评估日期：2026-10-31。结论：值得申请，尚不能认定已满足全部条件，更不保证获批。

| 条件 | 当前情况 |
| --- | --- |
| 自有源码和开源许可 | Windows 客户端及共享核心均为 MIT，已公开；Rust 依赖声明清单见第三方文档 |
| 维护和构建来源 | GitHub Actions 可追溯构建；Windows 27 项测试、实际 UI 和安装测试通过 |
| 已发布可执行程序 | 提供明确标注未签名的 0.2.1 版本；永久 Release：https://github.com/Infinifar/rustclipsync-desktop/releases/tag/v0.2.1 |
| 功能和隐私说明 | README、Code signing policy 已准备，包含自动剪贴板传输及服务端可读正文说明 |
| 维护者 MFA 和审批角色 | 角色已拟定，GitHub API 未返回可验证的 MFA 状态；须维护者确认 GitHub 和 SignPath 双重验证 |
| 项目信誉 | 新建项目、公开使用记录较少，是否达到可验证信誉要求由基金会决定，没有承诺通过 |
| 微软运行组件 | 自包含包包含第三方运行文件；是否符合 System Libraries 例外、安装器签名流程仍需确认 |

公开前使用官方 Gitleaks 8.30.1 扫描两仓库全部本地 Git 历史：Windows 36 个提交、核心 4 个提交，均未发现命中；这不等同于完整安全审计。

## 可用于申请的项目介绍

Project: RustClipSync Desktop

Repository: https://github.com/Infinifar/rustclipsync-desktop

Shared core: https://github.com/Infinifar/rustclipsync-core

License: MIT

Maintainer: Infinifar (https://github.com/Infinifar)

Description: A Windows 11 native text clipboard synchronization client with a WinUI 3 interface and a Rust background agent. It connects only to a user-configured HTTPS clipboard service for synchronization. Sensitive synchronization is disabled by default. Received marked sensitive text can be kept in an encrypted inbox or explicitly configured for automatic clipboard application. The project excludes marked sensitive content from ordinary application history and suppresses its own clipboard events. Local state and history are encrypted; synchronization is not end-to-end encrypted.

Build: GitHub Actions Windows workflow, pinned dependencies/actions, Rust core source snapshot with SHA-256 validation. Tested installer source commit: 165f7959d519c7438425e46df168928ad09cd232; workflow run: https://github.com/Infinifar/rustclipsync-desktop/actions/runs/41211210740

Distribution: Per-user Inno Setup EXE, with uninstallation. The package carries .NET, Windows App SDK and Microsoft Visual C++ runtime components. We request confirmation of the System Libraries exception and a supported signing process for the installer, uninstaller and temporary setup copies. We will not re-sign third-party DLLs under the project's certificate.

Signing policy: https://github.com/Infinifar/rustclipsync-desktop/blob/main/CODE-SIGNING.md

Contact email: to be supplied by the maintainer in the application; do not use a GitHub noreply address as contact.

## 维护者需要完成的外部步骤

1. 确认 GitHub 双重验证已开启，并在 SignPath 账户启用双重验证。
2. 在 https://signpath.org/apply 填写申请，使用真实可收信邮箱；如实说明项目尚新和运行组件情况。当前尚未代为提交或发送邮件。
3. 获批后安装官方 GitHub App，仅授予所需仓库访问权限，设置项目、产物配置及正式签名审批。
4. 提供非敏感项目标识，将 API Token 直接配置到 Actions Secret。再接入实际签名工作流，验证后发布首个签名版本。

依据：https://signpath.org/terms 和 https://docs.signpath.io/trusted-build-systems/github 。
