# 渠道：把聊天平台接到 Aura

渠道是插件后台服务里的一段适配：连上一个聊天平台（QQ、飞书、微信、企业微信、钉钉、Telegram、Slack、Discord，或 OneBot、Satori 这类通用协议），把用户发给机器人的消息交给 Aura，再把 Aura 的回复发回去。Aura 不认识任何平台：平台的连接、鉴权、令牌、消息格式都在适配里。

## 先查平台当前的文档

平台的接口、限额和审核规则常改。写适配前先读这个平台**现在**的开放平台文档，优先用平台官方的 SDK（Python、Node.js 都有的用对应语言的）。要弄清：

- 怎么收消息：长连接（WebSocket、长轮询、Stream 之类，Aura 这边不用公网地址）还是回调地址（平台向一个公网 HTTPS 地址推送，要用户自己准备地址或转发）。能选就选长连接，并告诉用户为什么。
- 私聊、群聊各能收到什么（很多平台的群里只推送 @ 机器人的消息）。
- 怎么发消息：被动回复（带原消息编号，通常有时限，例如几分钟内）还是主动发送（通常有次数限额）。Aura 的一回合可能要几分钟，被动回复超时后要改用主动发送。
- 令牌多久过期、怎么刷新；一条消息最多几个字符；支持不支持 Markdown。
- 机器人要用户在平台那边做什么：建应用、拿 AppID 和密钥、开权限、加白名单 IP、上线审核。写进插件的 `description` 或告诉用户。

## 连接平台

- 用平台官方的 SDK。没有 SDK 时用语言自带的：Node.js 22 起有全局的 `WebSocket` 和 `fetch`；Python 标准库有 `urllib`，没有 WebSocket 客户端，要用户装平台的 SDK 或 `websockets`。不要自己实现 WebSocket 这类协议。
- 会话的终端没有批准联网时，Aura 给终端里的命令设了一个拒绝代理，连本机地址也走它，所以在终端里试连平台（包括本机的假平台）会失败，这不是适配的问题。后台服务按用户自己的网络设置运行，不受这个限制：装好、信任、打开后台服务以后，用 `plugins.logs` 看它连上没有。

## 清单

```toml
schema_version = 2
name = "qq-bot"
version = "0.1.0"
description = "把 QQ 机器人接到 Aura"
capabilities = ["session.send", "notify"]

[service]
command = "python"
arguments = ["${PLUGIN_ROOT}/service/main.py"]
network = ["api.sgroup.qq.com", "bots.qq.com"]

[[channels]]
id = "qq-main"
platform = "qq"
title = "QQ 机器人"

[[settings]]
key = "app_id"
title = "AppID"
required = true

[[settings]]
key = "app_secret"
title = "AppSecret"
kind = "secret"
required = true
```

- `[[channels]]`：最多 10 个，要有 `[service]`。`id` 用小写字母、数字、`-` 和 `_`，最多 40 个字符，不能重复；`platform` 是 `qq`、`feishu`、`wechat`、`wecom`、`telegram`、`dingtalk`、`slack`、`discord`、`onebot`、`satori`、`other` 之一；`title` 一行，最多 40 个字符，显示在插件页和新会话的标题里。
- `message.in` 要 `session.send`。
- 平台的密钥、令牌写成保密设置（`kind = "secret"`），服务从 `AURA_SECRET_<键的大写>` 读。
- `network` 写全它会连的主机。

## 收到消息：`message.in`

平台推来一条用户的文字消息，就发一个请求（这页的例子为了好读分成了几行，发的时候每条照常是一行）：

```json
{
  "type": "request",
  "id": 7,
  "method": "message.in",
  "params": {
    "channel": "qq-main",
    "chat": { "kind": "private", "id": "8F2A…" },
    "user": { "id": "8F2A…", "name": "老王" },
    "message_id": "ROBOT1.0_…",
    "text": "帮我看看今天的提交",
    "reply_to": "ROBOT1.0_…"
  }
}
```

| 字段         | 规则                                                                                   |
| ------------ | -------------------------------------------------------------------------------------- |
| `channel`    | 清单里的渠道 `id`                                                                      |
| `chat.kind`  | `private`（和一个人）或 `group`                                                        |
| `chat.id`    | 平台给这个聊天的编号：私聊一般是对方的编号，群是群的编号。一行，最多 128 个字符        |
| `user.id`    | 发消息的人在平台上的编号，同上。白名单按它核对                                         |
| `user.name`  | 平台给的名字，可省，最多 64 个字符                                                     |
| `message_id` | 平台给这条消息的编号，一行，最多 128 个字符。同一条推送两次用同一个编号，Aura 只算一次 |
| `text`       | 消息的文字，不能为空，最多 16000 个字符。去掉 @ 机器人的那段；图片、语音这些先不转     |
| `reply_to`   | 可省：用户引用回复的那条消息的编号                                                     |

编号原样照抄平台给的，不要自己拼。机器人自己发的消息、平台的系统通知不要转。

回答：

| `result`                                                                                   | 意思                                                                       | 适配做什么                                         |
| ------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------- | -------------------------------------------------- |
| `{"accepted": true, "session_id": …, "message_id": …, "queued": false, "repeated": false}` | 进了会话；`queued` 为真表示前面有回合在跑，排着队                          | 等 `reply`                                         |
| `{"accepted": true, "session_id": …, "queued": false, "repeated": true}`                   | 这条已经收过了                                                             | 什么都不用做                                       |
| `{"accepted": true, "command": "help", "repeated": false}`                                 | 是一条命令，Aura 执行了（聊天已经有会话时带 `session_id`），见下面的“命令” | 等 `reply`                                         |
| `{"accepted": true, "command": "help", "repeated": true}`                                  | 这条命令已经执行过了                                                       | 什么都不用做                                       |
| `{"accepted": false, "reason": "user_not_allowed"}`                                        | 这个人不在白名单里                                                         | 什么都不回。插件页会列出他，用户可以一键加进白名单 |
| `{"accepted": false, "reason": "no_scope"}`                                                | 用户还没给插件可以进的会话                                                 | Aura 自己会发一条 `reply` 告诉对方去哪里设置       |

出错（`error`）时 `code` 是 `channel.unknown`、`channel.chat_invalid`、`channel.user_invalid`、`channel.message_id_invalid`、`channel.text_invalid`、`channel.reply_to_invalid`、`protocol.bad_params`，或 [service.md](service.md) 里的通用代码。写进日志，不要重试同一条。

同一个插件的 `message.in` 按发出的顺序一条一条交给 Aura。

## 消息去哪个会话

由 Aura 决定，适配不用管：

- 用户在插件页给插件选会话范围（工作区或单个会话）和每个渠道的白名单。适配和模型都改不了它们。
- 一个聊天（渠道 + 私聊或群 + 聊天编号）一直用同一个会话；那个会话不在范围里了，就用插件的聊天最近用过的、范围里点名的、范围里工作区最新的；都没有就在范围里第一个工作区新建一个，标题是“渠道名 · 对方名字”（没有名字用对方的编号；群是“渠道名 · 群 群的编号”）。
- 新会话的权限模式和模型是用户新会话的默认值。
- 会话里这条消息标明来自哪个插件、哪个平台、谁发的。

## 命令

聊天里打的“/命令”由 Aura 来认，适配把文字原样交给 `message.in`，不要自己解析：

- 只认整条消息开头的“/命令名”，命令名后面是空白或结尾；“/命令名@机器人名”（Telegram 群里的写法）的 @ 部分去掉。不是命令的名字整条当普通消息交给会话，消息中间的“/”不管。
- 命令和电脑上输入框的同一份：Aura 自己的命令，用户的、插件带的和聊天当前会话所在工作区的提示命令（关掉的、有问题的不算），再加只在聊天里用的几个。
- 命令的结果 Aura 用 `reply` 发回这个聊天（`format` 是 `text`，`reply_to` 是命令那条消息）；开始一回合的命令等回合结束再发回复。
- 同一条命令消息再推一次，回 `repeated: true`，不再执行。Aura 在内存里记每个插件最近 500 条命令消息、最多 24 小时，Core 重启后不记得。
- 提示命令：模板里的 `$ARGUMENTS` 换成命令后面的文字（模板里没有就接在模板后面），当普通消息交给会话；回答和普通消息一样，多带 `command`。

| 命令                           | 做什么                                                                             | 插件要声明        |
| ------------------------------ | ---------------------------------------------------------------------------------- | ----------------- |
| `/help`                        | 列出这个聊天现在能用的命令                                                         | `session.send`    |
| `/plan 要做的事`、`/goal 目标` | 当前会话空闲时按这个模式开始一回合（Goal 不设预算）                                | `session.send`    |
| `/init [补充说明]`             | 当前会话空闲时初始化它的工作区（建立或维护 AGENTS.md）                             | `session.send`    |
| `/stop`                        | 停止当前会话正在运行的回合                                                         | `session.control` |
| `/compact [重点]`              | 有回合在运行就登记一次压缩，在回合的安全点执行；没有就单独压缩一次                 | `session.control` |
| `/model [编号]`                | 列出能用的模型；带编号就换当前会话的模型                                           | `session.control` |
| `/permission [编号]`           | 列出三种权限模式；带编号往严了切直接生效，往宽了切要插件打开远程批准，规则见“审批” | `session.control` |
| `/sessions`                    | 插件会话范围里的会话，新的在前，最多 20 个                                         | `session.read`    |
| `/switch 编号`                 | 把这个聊天换到 `/sessions` 列出的那个会话                                          | `session.control` |
| `/new [标题]`                  | 在范围里第一个工作区新建会话，这个聊天换过去                                       | `session.control` |
| `/status`                      | 当前会话的标题、工作区、模型、权限模式、运行情况、排队的消息                       | `session.read`    |

- `/approve`、`/approve-session`、`/deny` 决定交给这个聊天的审批，见下面的“审批”。
- 电脑上打开面板或设置页的命令（`/context`、`/tools`、`/memory`、`/files`、`/diff`、`/terminal`、`/process`）不在 `/help` 和菜单里，打了回一句要在电脑上用。
- 插件没声明那项能力的命令也不在 `/help` 和菜单里，打了回原因。
- “当前会话”是这个聊天下一条消息会去的那个会话；还没有时，`/status` 这类命令回一句，`/plan`、`/goal`、`/init` 和提示命令像普通消息一样先建一个。

### 命令菜单

平台能显示命令菜单时（例如 Telegram 的 `setMyCommands`），在 `ready` 里给这个渠道报 `command_menu: true`。Aura 在服务启动后发一次，之后命令变了再发：

```json
{
  "type": "commands",
  "channel": "tg-main",
  "commands": [
    {
      "name": "plan",
      "title": "Plan 模式",
      "description": "按 Plan 模式开始一轮：先梳理方案，由你决定何时实施",
      "argument_hint": "要做的事"
    },
    { "name": "help", "title": "命令帮助", "description": "列出这个聊天能用的命令" }
  ]
}
```

- 每次都是整份菜单，没有参数的命令不带 `argument_hint`。按平台的接口整份换掉。
- 平台不接受的名字跳过（例如 Telegram 只认小写字母、数字和下划线，最多 32 个字符），超出平台条数上限的从后面去掉：没进菜单的命令照样能打。
- 菜单里是能在聊天里用的 Aura 命令、只在聊天里用的命令（`/approve`、`/approve-session`、`/deny` 只在插件打开远程批准时列）、用户和插件的提示命令。工作区的提示命令随会话变，不进菜单，`/help` 里有。

## 发回去：`reply`

这一回合结束后 Aura 发：

```json
{
  "type": "reply",
  "channel": "qq-main",
  "chat": { "kind": "private", "id": "8F2A…" },
  "reply_to": "ROBOT1.0_…",
  "text": "今天有 3 个提交：……",
  "format": "markdown"
}
```

- `reply_to` 是它回答的那条消息的 `message_id`（Aura 自己的提示也带着）。
- `format` 是 `markdown`（回合的最终回复，渠道在 `ready` 里报了 `markdown`）或 `text`（Aura 的提示，例如出错、被停止、没有会话范围；或者渠道不支持 Markdown，Aura 已经转成纯文字）。
- `markdown` 是常见的 CommonMark 写法。平台的 Markdown 和它不一样时（例如 Telegram 的 MarkdownV2 要转义很多符号），要么在 `ready` 里报 `markdown: false` 收纯文字，要么在适配里转成平台的写法。
- 比 `ready` 报的 `max_text_chars` 长时，Aura 分好段：每段一个 `reply`，带 `part`（第几段，从 1 起）和 `parts`（一共几段），按顺序发出去就行。被动回复只能回一次的平台，后面几段改用主动发送。
  - 长度按字符（Unicode 码位）数。平台按别的数法算时（例如按 UTF-16 算，一个表情算两个），报小一些的上限。
  - 代码块被分在两段之间时，Aura 在前一段末尾补上结束的围栏，在后一段开头重新打开。
- 发不出去（被动回复超时、令牌过期）先按平台规矩补救（换主动发送、刷新令牌重试），再不行写日志。
- 一个回合拿走了几条排队的消息时，每条消息都会收到这一回合的回复（`reply_to` 各不相同，`text` 一样）：适配可以只发一次。
- Aura 替一个插件的聊天最多记着 100 条没发的回复，每条最多 24 小时；Core 重启后，没发出的回复就不再发。

## 审批：`approval`

用户在插件页给插件打开“远程批准”后，聊天里开始的回合要用户批准一个操作时，Aura 把这条审批交给那个聊天：

```json
{
  "type": "approval",
  "channel": "qq-main",
  "chat": { "kind": "private", "id": "8F2A…" },
  "approval": {
    "id": "3",
    "title": "写入文件",
    "detail": "写结果\nagent-result.txt",
    "decisions": ["approve", "approve_session", "deny"]
  },
  "text": "Aura 请你决定（编号 3）：写入文件\n写结果\nagent-result.txt\n回复 /approve 批准这一次，/approve-session 本会话都允许，/deny 拒绝。"
}
```

- 只交给私聊，只交给这个聊天开始（或排着队）的回合的审批，插件要声明 `session.control`。一个插件同一时间只交出一条，后面的排队；交出的那条在电脑上处理了、过期了或回合没了，就交下一条。
- `id` 是 Aura 给的短编号；`title` 是要做的事；`detail` 是模型的调用说明和它要动的东西（文件、命令、网址），截短过，可能是空的；`decisions` 是聊天里能给的决定：`approve`（仅此一次）、`approve_session`（本会话都允许）、`deny`（拒绝）。
- 有些审批在聊天里只能拒绝（`decisions` 只有 `deny`）：只能允许一次的（永久删除、写工作区外面、初始化工作区这类），和只能按回合允许的（MCP 服务器和插件的工具）。批准要在电脑上。
- 平台有按钮时（`ready` 里报了 `buttons`），按 `decisions` 画按钮，按下时把对应的命令当一条消息交给 `message.in`：文字是 `/approve 3`、`/approve-session 3` 或 `/deny 3`，`message_id` 用平台给这次按下的编号（每次按下都不一样）。没有按钮的平台把 `text` 发出去就行。
- 服务重启后，交出去还没决定的那条会再交一次，编号是新的。
- 适配只转达：不要替用户回 `/approve`，也不要把审批转给别的聊天或别的人。

聊天里决定：

| 命令                      | 做什么                           | 插件要声明        |
| ------------------------- | -------------------------------- | ----------------- |
| `/approve [编号]`         | 批准交给这个聊天的审批，仅此一次 | `session.control` |
| `/approve-session [编号]` | 批准，本会话里同样的操作都允许   | `session.control` |
| `/deny [编号]`            | 拒绝                             | `session.control` |

这几条同时满足才算数，否则 Aura 回原因、什么都不做：

- 插件的“远程批准”打开着（关着时这三个命令不在菜单和 `/help` 里）；
- 消息来自白名单里的人的私聊，这个聊天有交出去的审批；带了编号就要和它对上；
- 交出后满 3 秒；
- 这条审批在聊天里能给这个决定；
- 同一条消息（同一个 `message_id`）只算一次。

算数后 Aura 照电脑上点按钮的方式记下决定，会话里那次工具调用标明“经插件 X 由 QQ 用户 老王 批准（仅此一次）”，再回聊天一句“已批准（仅此一次）。”之类的话。

`/permission 编号` 往宽了切（例如从只读切到完全访问）也按这些规则：远程批准打开着、私聊、这个聊天上一次 `/permission` 列出模式后满 3 秒、同一条消息只算一次；远程批准关着时回一句要在电脑上切。往严了切直接生效。

## 通用协议

- **OneBot v11**（`platform = "onebot"`）：一些 QQ 客户端框架提供的协议。正向 WebSocket（服务连框架）最省事：私聊 `message_type = "private"`、群 `"group"`；`user_id`、`group_id`、`message_id` 是数字，转成文字交给 Aura；发消息用 `send_private_msg`、`send_group_msg`。框架的地址和访问令牌写成设置。
- **Satori**（`platform = "satori"`）：跨平台的聊天协议，事件经 WebSocket 推送，`channel.id`、`user.id`、`message.id` 原样用；发消息用 `message.create`。
- 用户自己的框架、网关或别的协议：`platform = "other"`。

## 测试

- 先在插件文件夹外写一个假平台（本机的 HTTP 或 WebSocket 服务），让适配连它：推一条私聊、一条群聊、同一条推两次、一个不在白名单里的人，看 `plugins.logs` 和会话。
- 审批：请用户打开远程批准、把会话切到只读，让回合写一个文件；看审批是否交到那个私聊，3 秒后回 `/approve`，回合是否接着做完。
- 示例插件 `examples/fake-chat` 就是这样一套：一个本机假平台，一个用 Python 帮助库写的适配，审批整段转给私聊。
- 真平台的测试留给用户：告诉用户在平台那边要做的设置、在插件页要填的设置、要加进白名单的人。
