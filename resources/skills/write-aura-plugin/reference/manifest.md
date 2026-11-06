# aura-plugin.toml

TOML 文件，放在插件文件夹最外层。不认识的键会让整个清单无效。表（`[limits]`、`[panel]`、`[service]`、`[[channels]]`、`[[settings]]`）要写在所有顶层键之后。

```toml
schema_version = 2
name = "usage-panel"
version = "0.1.0"
description = "显示本会话的用量"
author = "你的名字"
homepage = "https://example.com/usage-panel"
capabilities = ["session.read"]

[limits]
memory_mb = 512
processes = 8

[panel]
title = "本会话用量"
entry = "panel/index.html"
network = []

[service]
command = "${PLUGIN_ROOT}/bin/bot.exe"
arguments = ["--quiet"]
network = ["api.sgroup.qq.com"]

[[settings]]
key = "app_id"
title = "App ID"
required = true

[[settings]]
key = "app_secret"
title = "App Secret"
kind = "secret"
required = true
description = "在 QQ 开放平台的开发设置里"
```

## 顶层

| 键                   | 必填 | 规则                                                                                                     |
| -------------------- | ---- | -------------------------------------------------------------------------------------------------------- |
| `schema_version`     | 是   | 1 或 2。写了 `capabilities`、`[panel]`、`[service]`、`[[channels]]`、`[[settings]]` 中任何一项就必须是 2 |
| `name`               | 是   | 小写字母、数字和 `-`，以字母开头，最多 32 个字符；插件的身份                                             |
| `version`            | 是   | 一行，最多 32 个字符                                                                                     |
| `description`        | 否   | 显示在插件页                                                                                             |
| `author`、`homepage` | 否   | 一行文字                                                                                                 |
| `capabilities`       | 否   | 面板和后台服务要的会话能力，见下表；默认一个都没有                                                       |

一个插件至少要带来一样东西：Skill、提示命令、MCP 服务器、Hook、面板、后台服务或主题（`theme.toml`，见 [theme.md](theme.md)）。

## capabilities

| 名字              | 能做什么                                                                                               |
| ----------------- | ------------------------------------------------------------------------------------------------------ |
| `session.read`    | 读会话的公开状态：标题、模型、用量、待办、上下文占用                                                   |
| `session.events`  | 收到会话事件：回合开始和结束、最终回复、出错的原因、等待审批、每次模型请求的开始和结束、工具调用、压缩 |
| `session.send`    | 替用户在会话里发消息，消息标明来自这个插件                                                             |
| `session.control` | 停止当前回合、新建会话、切换权限模式；用户打开远程批准后，私聊里的人决定交给他的审批                   |
| `notify`          | 显示提示                                                                                               |
| `status`          | 在状态栏显示一条                                                                                       |
| `commands`        | 运行插件自己 `commands/` 里的提示命令，作为插件的消息发进会话；之后照常走审批                          |
| `settings`        | 读写插件自己的文字设置；保密设置只有用户在插件页填                                                     |

只写用到的。写了不认识的名字，`plugins.validate` 会列出能用的名字。

## [limits]

插件的每个进程树（MCP 服务器、Hook、后台服务）都在这个限额里运行。

- `memory_mb`：64 到 16384，默认 2048。
- `processes`：1 到 512，默认 64。

## [panel]

右侧栏里由插件自己的页面画的一个标签。

- `title`：必填，一行，最多 40 个字符。
- `entry`：默认 `panel/index.html`；必须是 `panel/` 文件夹里的 `.html` 文件，用 `/` 分隔，不能有 `..`。页面和它用到的文件都放在 `panel/` 里。
- `network`：面板能连的地址，最多 20 个，每个是 `https://主机` 或 `wss://主机`，可带端口，不带路径和 `*`。默认一个都不能连。

## [service]

插件打开时 Aura 一直运行着的一个程序。

- `command`：插件里的程序（用 `${PLUGIN_ROOT}` 开头，例如 `${PLUGIN_ROOT}/bin/bot.exe`），或 `PATH` 上的程序名。程序要真的在插件文件夹里。
- `arguments`：最多 64 个。只有 `${PLUGIN_ROOT}` 会换成插件文件夹，其他 `${...}` 不会展开。
- `network`：它会连的主机名或网址，最多 20 个，只给用户看；它以用户的身份运行，Aura 拦不住它连别处，所以要写全、写准。

它和 Aura 之间的协议、能调的方法见 [service.md](service.md)。

## [[channels]]

后台服务连上的聊天平台账号（一个机器人一个渠道），最多 10 个；写了就要有 `[service]`。

```toml
[[channels]]
id = "qq-main"
platform = "qq"
title = "QQ 机器人"
```

- `id`：小写字母、数字、`-` 和 `_`，最多 40 个字符，不能重复。服务和白名单都用它。
- `platform`：`qq`、`feishu`、`wechat`、`wecom`、`telegram`、`dingtalk`、`slack`、`discord`、`onebot`、`satori`、`other` 之一，只显示给用户、交给服务；Aura 对每种平台的规矩都一样。
- `title`：必填，一行，最多 40 个字符。

适配怎么写见 [channels.md](channels.md)。

## [[settings]]

插件自己的设置，用户在插件页填。最多 20 项。

- `key`：小写字母开头，小写字母、数字和 `_`，最多 40 个字符，不能重复。
- `title`：必填，一行，最多 40 个字符。
- `kind`：`text`（默认）或 `secret`。保密的值存在 Windows 凭据管理器里，只交给插件的后台服务。
- `required`：默认 `false`。
- `description`：可选，一行，最多 200 个字符。
