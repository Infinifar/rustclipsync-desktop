# 构建与正式发布

统一入口为 Actions 中的 **Desktop CI / Release**（`.github/workflows/desktop.yml`）。
Windows 和 Linux 工作流仅作为可复用子工作流，不再分别响应主分支推送。

## 发布新版本

1. 更新客户端版本：Rust agent、Cargo.lock、WinUI 工程/关于、Win32 资源、
   Inno Setup、Qt 工程/应用版本和 Arch PKGBUILD。共享核心使用独立 API 版本。
2. 新建 `releases/v版本号.md`，填写本次发布说明。
3. 本地执行 `python3 scripts/release.py check` 和
   `python3 -m unittest discover -s scripts -p 'test_release.py' -v`。
4. 提交到 `main`。对应版本标签尚不存在时，工作流自动安排正式发布；已有标签
   的普通提交只运行 CI，不修改发布内容。

也可在 Actions 的同一个入口手动构建，勾选 **publish** 才会发布；发布来源限于
`main` 或与客户端版本一致的标签。推送匹配的 `vX.Y.Z` 标签也能触发完整流程。

## 同一次运行完成的工作

- 检查所有版本元数据一致，验证发布文件收集和清理规则。
- 并行执行 Windows 原生构建、剪贴板/IPC/界面/安装测试，以及四种 Linux
  发行版的构建、真实剪贴板/密钥环/界面/安装检查和独立 Wayland 测试。
- 收集一个 Windows EXE、两个 DEB、一个 RPM 和一个 Arch 包，逐项检查各
  构建提供的 SHA256SUMS；缺失、重复或校验不符时不发布。
- 建立固定到本次提交的草稿 Release，上传完整安装包和统一 SHA256SUMS，
  确认资产名称与大小后转为正式版本并设为 Latest。
- 发布成功后清理本仓库已完成的失败和超时运行及其产物。成功、取消、正在
  运行和当前运行保留；手动发布可关闭 **cleanup_failed**。

脚本可恢复同一提交的未发布草稿，不移动既有版本标签或替换已发布资产。
提交推进期间，Release 仍指向实际通过检查的提交。

## 权限与签名

使用 Actions 自带的短期 `GITHUB_TOKEN`，无需另外配置发布密钥。
构建仅需读取源码，发布任务单独取得 `contents: write`，清理任务单独取得
`actions: write`。

正式发布与代码签名分别管理。当前 Windows 安装包未签名，发布说明明确标注；
SignPath 获批并接入后，须先签名和验证，再生成最终校验和。见
[签名政策](CODE-SIGNING.md)。
