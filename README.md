# Aura

Aura is a Windows desktop app for working on code with AI models: you chat with a model inside a
workspace, and it reads, edits and runs things there under permissions you choose. This document
is in Chinese, like Aura's interface.

Aura 是一个在 Windows 上用 AI 模型写代码的桌面应用。你在一个工作区里和模型对话，模型在你选定的权限下读文件、改代码、运行命令、上网查资料，每一步都显示在会话里，需要你同意的操作会先问你。

## 能做什么

- **工作区与会话**：一个文件夹就是一个工作区，每个工作区里可以有多个会话；会话可以归档、恢复、删除，也可以从某一轮分出一个新会话继续。
- **模型**：接入 DeepSeek 等提供 OpenAI 兼容接口或 Anthropic Messages 接口的服务，每个会话可以选模型和思考程度；上下文快满时自动压缩，也可以用 `/compact` 手动压缩。
- **权限模式**：只读、工作区读写、完全访问三档；超出当前模式的操作会先问你，可以只同意这一次，也可以在本会话内一直同意。
- **工具**：读写和搜索文件、PowerShell 与 CMD、终端、Git、网页搜索与抓取、HTTP 请求、内置浏览器、记忆，以及你自己接入的 MCP 服务器、Skill、插件和 Hook。
- **任务方式**：`/plan` 先出方案、`/goal` 定目标、`/init` 为工作区生成说明文件；任务进行中可以继续发消息排队，也可以随时停止。

## 安装

1. 从 [Releases](https://github.com/mubai0628/aura/releases) 下载最新的 `Aura-Setup-<版本>-x64.exe`，可以用同一页的 `SHA256SUMS` 核对文件。
2. 双击运行。安装程序没有 Windows 代码签名，第一次运行时 Windows 可能提示“已保护你的电脑”：点“更多信息”，再点“仍要运行”。
3. 默认装到 `%LOCALAPPDATA%\Programs\Aura`，不需要管理员权限；装好后从开始菜单打开 Aura。

系统要求：Windows 11 x64。Aura 自带 PowerShell 7，Git 功能使用你自己安装的 Git for Windows（2.40 或更新）。

## 第一次使用

1. 打开“设置 → 模型”，添加一个服务（填地址和 API Key），选好要用的模型。API Key 保存在 Windows 凭据管理器里，不写进任何文件。
2. 回到主界面，新建一个工作区（选一个文件夹），在输入框里说出你要做的事。
3. 在输入框下方切换权限模式和模型；需要你同意的操作会出现在会话里等你决定。

更多说明见 [用户文档](docs/user/README.md)。

## 更新、回退与卸载

- **更新**：Aura 会定期检查 GitHub 上的新版本，也可以在“设置 → 关于与更新”里手动检查。下载好的新版本在下次启动时试运行，启动成功才替换旧版本，启动失败会自动回到原来的版本。
- **回退**：“设置 → 关于与更新”里可以回到上一个版本，并可选择同时恢复更新前的数据。
- **卸载**：在 Windows“设置 → 应用”里卸载 Aura。默认保留你的数据（会话、设置、记忆），卸载时可以选择一并删除。

## 数据与隐私

Aura 只把你的消息和模型需要的内容发给你配置的模型服务；联网工具只在你允许的范围内访问网络。会话与设置保存在 `%LOCALAPPDATA%\Aura`，全局记忆在 `%USERPROFILE%\.aura\memories`，工作区记忆在工作区的 `.memories` 文件夹里，密钥在 Windows 凭据管理器里。Aura 不收集使用数据。详见 [数据与隐私](docs/user/data-and-privacy.md)。

## 尚未实现与已知限制

- **尚未实现**：子代理（多个智能体分工）、在沙箱里运行命令、更严格或带引导的安全档位、让模型起草工具。设计或代码里有过这些内容，但当前版本没有接通。
- **命令以你的 Windows 账户运行**，不在沙箱里，能改动工作区以外的文件、能联网。工作区读写下命令直接运行，删除文件和写到工作区以外的命令每次都问，下载的命令每个会话问一次；只读下每个会话第一次运行命令前会先问你。这些只按命令文字识别，脚本和程序内部做什么看不到。
- 只支持 Windows 11 x64；安装程序没有代码签名（见上文“安装”）。

完整列表见 [尚未实现与已知限制](docs/user/limitations.md)。

## 从源码构建

见 [开发文档](docs/dev/README.md)。

## 许可证

[MIT](LICENSE)。Aura 使用的第三方组件及其许可证见每个版本附带的 `THIRD-PARTY-NOTICES.txt`。
