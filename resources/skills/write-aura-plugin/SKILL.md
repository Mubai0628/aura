---
name: write-aura-plugin
description: 和用户一起写 Aura 插件（面板、后台服务、聊天机器人渠道、主题、Skill、提示命令、Hook、MCP 服务器）：在工作区里写好，用 plugins.validate 检查、plugins.dev_install 装成开发中的插件、plugins.logs 读报错。用户要做插件、给 Aura 加面板，或把 QQ、飞书、微信、Telegram 这类聊天机器人接到 Aura 时用。
---

# 写 Aura 插件

插件是一个文件夹：最外层是 `aura-plugin.toml`，旁边是它带来的各部分。你在用户的工作区里写这个文件夹，写好后装成“开发中的插件”给用户试。

## 你能做的和不能做的

- 能：在工作区里建文件夹、写文件；用 `plugins.validate` 检查；用 `plugins.dev_install` 装成开发中的插件；用 `plugins.logs` 读它的报错。
- 不能：信任插件、打开它的开关、填它的设置、选它能用的会话或外部用户。这些只有用户在 设置 → 插件 里能做。装好后告诉用户去那里查看并信任，不要说你已经替用户做了。
- `capabilities` 只写插件真用到的，用户信任时逐条看到。
- 插件里的程序以用户的身份运行。不写下载并运行别的程序、读与插件无关的用户文件、把数据发到没在清单里写明的地址的代码。
- 用户的密钥、令牌只能写成保密设置（`kind = "secret"`），由用户在插件页填；不要写进任何文件。
- 插件绝不能替用户批准。只有用户本人的回复算批准；插件里不要写自动批准，也不要把审批交给别人。远程批准时，适配只把 Aura 交来的审批转给那个私聊，把那个人的回复原样交给 `message.in`。

## 步骤

1. 先问清楚：插件做什么、显示在哪里、要连哪些外部服务、要用户填哪些设置。
2. 在工作区里建文件夹，例如 `plugins/<名字>/`。名字只用小写字母、数字和 `-`，以字母开头，最多 32 个字符。它是插件的身份，装上后不要改。
3. 写 `aura-plugin.toml`（见 [reference/manifest.md](reference/manifest.md)）和要的各部分（见 [reference/parts.md](reference/parts.md)）；面板的页面怎么写、能调什么，见 [reference/panel.md](reference/panel.md)；主题见 [reference/theme.md](reference/theme.md)；后台服务和它的协议见 [reference/service.md](reference/service.md)；把聊天平台接进来见 [reference/channels.md](reference/channels.md)。
   - 后台服务用帮助库写：把 `sdk/python/aura_plugin.py` 或 `sdk/node/aura-plugin.mjs` 原样复制进插件文件夹（例如 `service/`），用 Shell 的复制命令，或读出来原样写进去，不要改它。
   - 接聊天平台照着 `examples/fake-chat` 写：一个本机的假聊天平台和它的适配。先用假平台把流程跑通，再接真平台。
4. 调 `plugins.validate`，参数 `{"path": "plugins/<名字>"}`（相对工作区根目录，不能是链接）。逐条改掉 `problems`；`warnings` 里是 Aura 不会用到的部分。
5. 没有问题后调 `plugins.dev_install`，参数相同。只读模式下装不了，请用户切换权限模式。同一个文件夹再装一次什么都不变；名字被别的插件占了会被拒绝，换一个名字。
6. 告诉用户：去 设置 → 插件 查看它带来的内容并信任，再按需打开开关、填设置；带主题的，到 设置 → 环境 → 外观 选中它。
7. 用户说不好用时，先调 `plugins.logs`，参数 `{"name": "<名字>"}`，读它的 Hook、MCP 服务器、面板和后台服务最近的报错（保密值已抹掉），再改文件。

这三个工具可能不在你这一步的工具列表里：用 `tools.call` 调用，`tool` 写 `plugins.validate` 这样的名字，`arguments` 写参数；要看输入格式先用 `tools.describe`。

## 改了之后要不要重新信任

开发中的插件就在用户的文件夹里用，不复制：

- 改 `skills/`、`commands/`、`panel/` 里的文件或 `theme.toml`：不用重新信任，Skill 和命令下次列出时就是新的。
- 改 `aura-plugin.toml`、`hooks/`、`mcp.json` 或其他文件（包括后台服务的程序）：插件变成“内容已变化，需要重新信任”，重新信任前它带来的东西都不提供。改完提醒用户去插件页重新信任。
