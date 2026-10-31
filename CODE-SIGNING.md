# Code signing policy

状态：正在评估 SignPath Foundation，尚未提交申请、尚未获批，当前安装包未签名。本文不表示已经获得签名证书或服务。

## 项目和职责

RustClipSync Desktop 和共享 rustclipsync-core 使用 MIT 许可证。拟由 [Infinifar](https://github.com/Infinifar) 承担作者、审查及正式签名批准职责；最终角色以 SignPath 配置为准。外部贡献的代码和工作流变更须由维护者审查。GitHub 与 SignPath 双重验证必须由维护者确认启用，当前未核实。

## 签名范围

申请范围为本项目从公开源码构建的 RustClipSync.exe、rustclipsync-agent.exe、自有托管程序集，以及安装程序；Inno Setup 卸载器和临时安装副本的处理需与 SignPath 确认可行流程。不得使用本项目的 SignPath Foundation 权限重新签署第三方 DLL。

仅提交能追溯到公开提交、构建工作流和版本的产物。第三方组件保留原签名和许可证。当前自包含微软运行组件是否符合 System Libraries 例外，需要如实申报并由 SignPath 确认；详见 [第三方组件](THIRD-PARTY.md)。

## 隐私

应用自身没有统计上报或广告功能。启动同步后，剪贴板内容按用户选择的方向、过滤和敏感策略发送至用户配置的 HTTPS 服务端；自动同步不需要逐条确认。服务端可读取正文，这不是端到端加密。敏感同步默认关闭。应用本地状态和历史加密保存，日志及诊断不保存剪贴板正文。用户明确显示敏感收件项时，正文才进入小窗；复制后其他应用可读取系统剪贴板。

安装器按用户安装，自启由用户显式开启，支持卸载；卸载保留配对和加密数据。Windows、微软运行组件及用户所选服务端另受其各自条款和隐私政策约束。

## 获批后的发布流程

1. 在固定源码提交上构建并测试自有程序。
2. 使用可信 GitHub 构建来源提交签名请求；每次正式签名由维护者人工批准。
3. 签署自有程序，再通过确认过的 Inno Setup 流程处理安装器及卸载器，添加可信时间戳。
4. 检验签名、发布者、版本及安装行为，再生成校验和并发布 GitHub Release。签名失败不能静默发布为“签名版”。

拟配置的参数：SIGNPATH_ORGANIZATION_ID、SIGNPATH_PROJECT_SLUG、SIGNPATH_SIGNING_POLICY_SLUG、SIGNPATH_ARTIFACT_CONFIGURATION_SLUG；API 凭据保存在 Actions Secret SIGNPATH_API_TOKEN 中。当前尚未配置，也没有启用空壳签名工作流。无需生成或上传 Android keystore，也不要把私钥、令牌发到聊天或提交源码。

获批并实际启用后，主页和发布页应加入官方要求的署名：

> Free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

届时签名证书发布者将是 SignPath Foundation，不能承诺显示 Infinifar；新应用的有效签名也不保证立即消除 SmartScreen 提示。

条件依据：[SignPath 条款](https://signpath.org/terms)、[GitHub 集成](https://docs.signpath.io/trusted-build-systems/github)。
