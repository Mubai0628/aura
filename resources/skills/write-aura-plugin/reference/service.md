# 后台服务：协议第 1 版

`[service]` 写的程序由 Aura 启动，在插件打开期间一直运行。它和 Aura 之间只有一条通道：程序自己的标准输入和标准输出，每行一个 JSON 对象（UTF-8，以 `\n` 结尾，一行最多 1 MiB）。标准错误只进插件的日志。

不要自己写分行、请求编号、等回应这些：把帮助库原样复制进插件文件夹用（Python 见 `sdk/python/aura_plugin.py`，Node.js 见 `sdk/node/aura-plugin.mjs`，都在这个 Skill 的文件夹里；文件开头写了怎么用）。下面是它们实现的协议，出错时用来对照。

## 什么时候运行

- 插件信任了、打开了，插件页上“后台服务”的开关打开了，必填的设置都填了：Aura 启动它。少一样它就不运行，插件页写原因。
- 用户改了插件的设置、插件自己用 `settings.set` 改了文字设置、插件的文件变了并重新信任：Aura 先让它退出，再按新的样子启动。
- 它退出了、30 秒内没发 `ready`、不再读标准输入：Aura 过 1、2、4……最多 60 秒再启动它；连续正常运行 5 分钟后从 1 秒重新算。
- 它说的协议版本不对：停下，不再启动，插件页写原因。
- 关掉开关、关掉插件、退出 Aura：Aura 发 `shutdown`，5 秒后结束它的整个进程树（退出 Aura 时不等）。

## 它运行的环境

- 以用户的身份运行，在插件 `[limits]` 的内存和进程数限额里。
- 当前文件夹是它的数据文件夹。数据只写在这里，**不要写进插件文件夹**：插件文件夹变了，插件就要重新信任。
- 环境变量：用户的环境，加上
  - `AURA_PLUGIN_NAME`：插件名。
  - `AURA_PLUGIN_DATA`：数据文件夹。插件卸载后 Aura 会删掉它。
  - `AURA_PLUGIN_PROTOCOL`：`1`。
  - `AURA_SECRET_<键的大写>`：每个填了值的保密设置，例如 `app_secret` 是 `AURA_SECRET_APP_SECRET`。不要写进日志、文件或发给清单里没写的地址。
  - `TEMP`、`TMP`：Aura 给它的临时文件夹。
  - `PYTHONDONTWRITEBYTECODE=1`：Python 不在插件文件夹里写 `__pycache__`。不要关掉它，插件文件夹一变就要重新信任。
- 文字设置的值在 `hello` 里，也可以用 `settings.get` 再读、用 `settings.set` 改。

## 一来一回

```text
Aura → 服务  {"type":"hello","protocol":1,"plugin":"qq-bot","channels":[{"id":"qq-main","platform":"qq","title":"QQ 机器人"}],"settings":{"app_id":"1024"}}
服务 → Aura  {"type":"ready","protocol":1,"channels":[{"id":"qq-main","max_text_chars":2000,"markdown":true,"groups":true}]}
服务 → Aura  {"type":"request","id":1,"method":"message.in","params":{...}}
Aura → 服务  {"type":"response","id":1,"result":{"accepted":true,"session_id":"ses_…","message_id":"evt_…","queued":false,"repeated":false}}
Aura → 服务  {"type":"reply","channel":"qq-main","chat":{"kind":"private","id":"u1"},"reply_to":"m-1","text":"你好！","format":"markdown"}
服务 → Aura  {"type":"log","level":"info","text":"已连上 QQ"}
Aura → 服务  {"type":"shutdown"}
```

### Aura 发给服务的

| `type`     | 字段                                                                                                                                | 说明                                                                                               |
| ---------- | ----------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------- |
| `hello`    | `protocol`、`plugin`、`channels`（清单里的 `id`、`platform`、`title`）、`settings`（文字设置的值）                                  | 第一行。收到后连上平台，再发 `ready`                                                               |
| `response` | `id`，`result` 或 `error`（`code`、`message`）                                                                                      | 回答服务的一个请求，`id` 原样带回                                                                  |
| `error`    | `code`、`message`                                                                                                                   | 服务的某一行读不懂、又不是能回答的请求时                                                           |
| `reply`    | `channel`、`chat`（`kind`、`id`）、`reply_to`（可能没有）、`text`、`format`（`markdown` 或 `text`）；分成几段时还有 `part`、`parts` | 要发回聊天的话，见 [channels.md](channels.md)                                                      |
| `commands` | `channel`、`commands`（每项 `name`、`title`、`description`，有参数的还有 `argument_hint`）                                          | 只发给 `ready` 里报了 `command_menu` 的渠道：平台命令菜单的整份内容，见 [channels.md](channels.md) |
| `approval` | `channel`、`chat`（`kind`、`id`）、`approval`（`id`、`title`、`detail`、`decisions`）、`text`                                       | 插件打开了远程批准时，交给私聊、等那个人决定的审批，见 [channels.md](channels.md)                  |
| `shutdown` | 无                                                                                                                                  | 尽快退出（几秒内）                                                                                 |

以后可能加新的 `type`：不认识的就忽略，不要退出。

### 服务发给 Aura 的

| `type`    | 字段                                                                              | 说明                                                                                                                                                                     |
| --------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `ready`   | `protocol`（写 `1`）、`channels`                                                  | 30 秒内发。`channels` 每个渠道一项：`id`、`max_text_chars`（平台一条消息最多几个字符，1 到 100000）、`markdown`、`buttons`、`command_menu`、`groups`（默认都是 `false`） |
| `request` | `id`（整数，或不超过 64 个字符的一行文字）、`method`、`params`（JSON 对象，可省） | 调 Aura 的方法，等 `response`                                                                                                                                            |
| `log`     | `level`（`info`、`warning`、`error`，默认 `info`）、`text`                        | 写进插件日志，一条最多 4000 个字符                                                                                                                                       |

- `ready` 之前的请求回 `protocol.not_ready`。
- 同时最多 16 个请求没回应，多的回 `protocol.busy`；还没回应的 `id` 再用回 `protocol.duplicate_id`。
- 写错的行：不是 JSON 回 `protocol.not_json`，缺字段或多字段回 `protocol.bad_message`，不认识的 `type` 回 `protocol.unknown_type`，太长的一行丢掉到下一个换行为止、回 `protocol.line_too_long`。

## 方法

| 方法             | 要的能力          | 参数                                                                             | 回答                                                                                                                       |
| ---------------- | ----------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| `notify`         | `notify`          | `text`（一行，最多 200 个字符）、`tone`（`info`、`success`、`warning`、`error`） | `{}`；10 秒内最多 3 条，多的回 `plugin_session.notice_too_many`                                                            |
| `status`         | `status`          | `text`（最多 40 个字符；不给就清掉）、`tone`                                     | `{}`。状态栏一条，服务停了 Aura 清掉                                                                                       |
| `settings.get`   | `settings`        | 无                                                                               | `{"values": {键: 值}}`，只有文字设置                                                                                       |
| `settings.set`   | `settings`        | `key`、`value`（空字符串清掉）                                                   | 改后的 `{"values": {键: 值}}`，和面板的 `auraPanel.setSetting()` 一样核对；之后 Aura 带着新值重启服务                      |
| `message.in`     | `session.send`    | 见 [channels.md](channels.md)                                                    | 见 [channels.md](channels.md)                                                                                              |
| `sessions.list`  | `session.read`    | 无                                                                               | `workspaces`（`workspace_id`、`name`）、`sessions`（`session_id`、`workspace_id`、`title`，新的在前，最多 200 个）、`more` |
| `session.state`  | `session.read`    | `session_id`                                                                     | 和面板的 `auraPanel.state()` 一样，见 [panel.md](panel.md)                                                                 |
| `session.events` | `session.events`  | `session_id`、`after`（可省）                                                    | 和面板的 `auraPanel.events()` 一样                                                                                         |
| `session.send`   | `session.send`    | `session_id`、`text`、`dedupe_key`（可省）                                       | 和面板的 `auraPanel.send()` 一样                                                                                           |
| `command.run`    | `commands`        | `session_id`、`name`（不带 `/`）、`arguments`、`dedupe_key`（后两个可省）        | 和面板的 `auraPanel.run()` 一样                                                                                            |
| `session.stop`   | `session.control` | `session_id`                                                                     | 和面板的 `auraPanel.stop()` 一样                                                                                           |
| `session.new`    | `session.control` | `workspace_id`、`title`、`dedupe_key`（都可省）                                  | `session_id`、`workspace_id`、`title`                                                                                      |

- 会话方法只作用于插件的**会话范围**：用户在插件页给它选的工作区和会话。范围外的回 `plugin_session.out_of_scope`，不说那个会话在不在；范围里点名的会话删了回 `plugin_session.session_missing`。
- `session.new` 不给 `workspace_id` 就建在范围里第一个工作区；标题默认是插件名，一行最多 200 个字符；同一个 `dedupe_key` 回同一个会话，键用在别的请求上回 `plugin_session.dedupe_key_reused`。新会话的权限模式和模型是用户新会话的默认值。
- 清单里没声明的能力回 `plugin_session.capability_missing`；插件没信任、关了、刚改过回 `plugin_session.not_ready`；Aura 还没准备好会话回 `aura.not_ready`；Aura 正在退出回 `aura.stopping`；参数不对回 `protocol.bad_params`；没有这个方法回 `protocol.unknown_method`；别的失败回 `aura.failed`。`message` 是给人看的原因，写进日志。
- 协议里没有改会话范围、白名单、开关、保密设置、权限模式和批准的方法：这些只有用户能改。白名单里的人在聊天里打的命令（例如 `/permission`、`/approve`）由 Aura 照规则执行，见 [channels.md](channels.md)。

## 日志

- 标准错误的每一行和 `log` 都进插件日志，Aura 先抹掉交给它的保密值；每分钟最多 120 行，多的记一条丢了几行。
- 用户说不好用时，用 `plugins.logs` 读。

## 写服务时

- `ready` 的 `channels` 写对平台一条消息最多几个字符、支不支持 Markdown。先发 `ready` 再连平台也行。
- Aura 按 `ready` 里每个渠道的 `max_text_chars` 分段，按 `markdown` 发 Markdown 或转好的纯文字：每个 `reply` 照原样发出去就行。`ready` 没报的渠道，Aura 不转格式，一段超过 100000 个字符才分。适配自己发的长消息用帮助库的 `split_text`、`splitText` 分。
- 收到 `shutdown` 就断开平台、退出。
- 和平台的连接断了自己重连，退避着来；不要因为平台出错就退出，那会让 Aura 也退避着重启它。
- 不要在 `reply` 和 `shutdown` 的处理函数里做长时间的事：帮助库一个接一个地跑它们。
- 处理函数登记之前收到的 `reply`、`commands`、`approval`，帮助库先存着（每种最多 100 条，多了丢最早的），登记时按收到的顺序交给它。报了 `command_menu` 的渠道，Aura 在 `ready` 之后马上就发命令菜单；先 `start()` 后登记也不会丢。
- 同时最多 16 个请求在等回答，帮助库替你排队，回应按请求自己的 `id` 对上。
