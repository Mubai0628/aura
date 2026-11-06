# 面板

面板是插件自己的网页，显示在会话右侧栏的一个标签里。清单里写 `[panel]`（见 [manifest.md](manifest.md)），页面和它用到的文件都放在 `panel/` 里。

用户从右侧栏的“+”打开它，每个会话一个；切到别的标签或会话时页面在后台继续运行，关掉标签页面就结束。页面做的每件事都只作用于打开它的那个会话。

## 页面在哪里

- 地址是 `aura-plugin://<插件名>/` 加 `panel/` 之后的路径：`panel/index.html` 是 `aura-plugin://usage-panel/index.html`。引用别的文件用相对路径，例如 `<script src="app.js">`。
- 只能读 `panel/` 里的文件，类型限于 `.html .htm .js .mjs .css .json .map .txt .svg .png .jpg .jpeg .gif .webp .ico .woff .woff2 .ttf .otf`，每个最多 16 MB。
- 文件必须是普通文件：链接、快捷方式、junction、硬链接都读不到；名字不能以点或空格结尾，也不能是 `con`、`nul` 这类设备名。
- 改了 `panel/` 里的文件，打开着的面板在文件停止变化约 0.3 秒后自动重新载入，不用重新信任。

## 页面能做的和不能做的

页面在只属于这个插件的沙盒里运行，每个文件都带着下面的内容安全策略：

- 脚本只能是 `panel/` 里的文件。不能写内联 `<script>`、`onclick=` 这类属性、`eval`、`new Function`，也不能运行 WebAssembly。
- 样式可以写在 `.css` 文件里，也可以内联。图片和字体用 `panel/` 里的文件或 `data:`，图片还可以用 `blob:`。
- `fetch`、`WebSocket` 只能连 `panel/` 自己和 `[panel] network` 里写的地址。别的请求会被拦下，记进插件的日志。
- 没有 Node；不能打开新窗口、跳出自己的地址、嵌别的网页（`<iframe>`、`<object>`）、提交表单、下载文件，也拿不到摄像头、麦克风、通知、剪贴板这类权限。
- 页面自己画背景；透明的地方露出侧栏的底色。
- 会话里的文字（标题、最终回复、待办）不可信：用 `textContent` 显示，不要用 `innerHTML`。

## window.auraPanel

```js
auraPanel.version; // 1
auraPanel.capabilities; // 清单里声明、用户已信任的能力
await auraPanel.state(); // 要 session.read
await auraPanel.events(after); // 要 session.events
await auraPanel.send(text, { dedupeKey: key }); // 要 session.send
await auraPanel.run(name, { arguments: text, dedupeKey: key }); // 要 commands
await auraPanel.stop(); // 要 session.control
await auraPanel.notify(text, { tone }); // 要 notify
await auraPanel.status(text, { tone }); // 要 status
await auraPanel.settings(); // 要 settings
await auraPanel.setSetting(key, value); // 要 settings
```

没声明的能力就没有对应的方法；用前先判断，例如 `typeof auraPanel.send === 'function'`。

### state()

```json
{
  "session_id": "ses_…",
  "title": "会话标题",
  "model_profile_id": "…",
  "model_name": "…",
  "activity": "idle",
  "waiting_for": "approval",
  "usage": {
    "requests": 3,
    "input_tokens": 120000,
    "cached_input_tokens": 90000,
    "output_tokens": 4000
  },
  "todo": [{ "id": "…", "title": "…", "status": "in_progress" }],
  "context": { "window_tokens": 128000, "used_tokens": 52000, "estimated": false },
  "cursor": 42
}
```

- `activity`：`idle`、`running` 或 `waiting`；在等时 `waiting_for` 是 `approval`、`question` 或 `context`。
- `todo` 是最新一个回合的待办，`status` 是 `pending`、`in_progress`、`completed`、`blocked` 或 `cancelled`。
- `model_profile_id`、`model_name`、`waiting_for`、`todo`、`context` 没有时不出现。
- `cursor` 是从现在起读事件的位置。

### events(after)

读 `after` 之后的事件。没有推送，页面自己定时读：先用 `state()` 拿 `cursor`，之后每隔几秒调 `events(cursor)`，每次换成返回的 `cursor`；`more` 为 `true` 时马上再读一次。

```json
{
  "events": [
    { "sequence": 43, "event": { "kind": "turn_started", "turn_task_id": "…" } },
    {
      "sequence": 44,
      "event": { "kind": "step_started", "turn_task_id": "…", "model_step_id": "…" }
    },
    {
      "sequence": 46,
      "event": {
        "kind": "step_ended",
        "turn_task_id": "…",
        "model_step_id": "…",
        "usage": { "input_tokens": 12000, "cached_input_tokens": 9000, "output_tokens": 300 }
      }
    },
    {
      "sequence": 46,
      "event": {
        "kind": "tool_call",
        "turn_task_id": "…",
        "model_step_id": "…",
        "tool_call_id": "…",
        "tool": "fs.write"
      }
    },
    {
      "sequence": 47,
      "event": { "kind": "approval_waiting", "turn_task_id": "…", "approval_id": "…" }
    },
    {
      "sequence": 60,
      "event": {
        "kind": "turn_ended",
        "turn_task_id": "…",
        "outcome": "completed",
        "final_reply": "……"
      }
    }
  ],
  "cursor": 60,
  "more": false
}
```

- `turn_started`、`turn_ended`：回合开始和结束。`outcome` 是 `completed`、`failed`、`cancelled` 或 `recovery_required`；回合被拿去恢复时没有 `outcome`。`final_reply` 只在回合完成且有回复时出现；`reason` 只在 `failed`、`recovery_required` 时出现，是 Aura 记的原因，最多 500 个字符。
- `step_started`：回合里的一次模型请求要发出去了。
- `step_ended`：模型回答完这次请求，`usage` 是服务商报的这次用量。请求没有得到回答（出错、被停下）时没有这一条。
- `tool_call`：这次回答要调的工具，每个一条，按回答里的顺序，跟在它的 `step_ended` 后面、`sequence` 相同。`tool` 是 Aura 里的工具名；参数和结果都不给。
- `approval_waiting`：回合在等用户批准。
- `compaction_started`、`compaction_ended`：压缩会话历史的开始和结束，带 `compaction_id`；`compaction_ended` 的 `outcome` 是 `completed`、`failed`、`cancelled` 或 `interrupted`。模型没写出摘要时，Aura 可能用自己的摘要补上：同一个 `compaction_id` 先来 `failed`，再来 `completed`。
- 以后可能加新的 `kind`：不认识的就跳过。

### send(text, { dedupeKey })

替用户在会话里发一条消息，消息标明来自这个插件。`text` 不能为空，最多 16000 个字符。正在跑回合时消息排在它后面。

返回 `{"message_id": "…", "queued": true, "repeated": false}`。

- `dedupeKey`：一行，最多 128 个字符。同一个键再发同样的文字，返回 `repeated: true`，不会多发；同一个键换了文字会被拒绝（`plugin_session.dedupe_key_reused`）。
- 不带 `dedupeKey` 时每次调用都是一条新消息，出错后重试可能发两遍；会重试的消息一定带上键。
- 发消息不带任何权限：回合照常按会话的权限模式走，要批准时等用户本人批准。

### run(name, { arguments, dedupeKey })

运行插件自己 `commands/` 里的一个提示命令（`name` 不带 `/`），作为插件的消息发进这个会话，和用户在输入框里打这个命令一样：模板里的 `$ARGUMENTS` 换成 `arguments`；模板里没有 `$ARGUMENTS` 时，`arguments` 空一行接在模板后面。返回同 `send()`。

- 只认这个插件自己带的、现在能用的提示命令，别的来源的同名命令不算。没有这个命令报 `plugin_session.command_unknown`；命令被关掉或要重新信任时报 `plugin_session.command_unusable`。
- 发出的消息和 `send()` 一样：标明来自插件，同一个 `dedupeKey` 不会发两遍，不带任何权限，回合里的工具调用照常按会话的权限模式走。
- 只要 `commands`，不用另外声明 `session.send`；它发不出命令以外的文字。

### stop()

停下正在跑的回合，返回 `{"stopped": true}`；没有在跑的回合时是 `false`。停下要等回合收尾，等太久会报错而回合其实已停：报错后用 `state()` 看一眼。

### notify(text, { tone })

在 Aura 的提示里显示一条，标题是“插件 <插件名>”。`text` 是一行不空的文字，最多 200 个字符。返回 `{"plugin": "…", "text": "…", "tone": "info"}`。

- `tone`：`info`（默认）、`success`、`warning` 或 `error`。`error` 的提示留到用户关掉，别的几秒后自己消失。
- 每个插件 10 秒内最多 3 条，多了报 `plugin_panel.too_many_notices`；同样的文字还显示着时不再多显示一条。
- 提示是给人看一眼的：要用户处理的事留在面板里，不要靠提示。

### status(text, { tone })

在左下角 Core 状态下面显示插件的一条状态，前面是插件名。`text` 是一行文字，最多 40 个字符；不带 `text`、`null` 或空白就是清掉。返回 `{"plugin": "…", "text": "…", "tone": "info"}`，清掉时没有 `text`。

- `tone` 同上，决定前面圆点的颜色。
- 每个插件只显示一条：几个面板都设了时显示最后设的那条。面板关掉，它设的那条跟着消失。
- 显示的内容每秒最多变一次，一秒内连着设几次只显示最后一次；放心按需要设，不用自己限频。
- 只放持续的状态（“已连接”“3 条未读”），不放一闪而过的进度。

### settings() 和 setSetting(key, value)

读写插件在清单 `[[settings]]` 里声明的文字设置，和用户在 设置 → 插件 里填的是同一份。都返回 `{"values": {"greeting": "你好"}}`，只列有值的文字设置；`setSetting` 返回改后的。

- `setSetting` 和插件页一样核对：键要是清单里声明的文字设置，值是一行文字，最多 2000 个字符，前后的空白去掉；空字符串清掉这个值。
- 保密设置读不到也改不了；会话范围、白名单、远程批准和后台服务的开关只有插件页能改。
- 改了以后，插件的后台服务会带着新值重启。

## 出错

方法失败时抛出 `Error`，`message` 是“代码: 说明”。

| 代码                                                                           | 意思                                                                                                 |
| ------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------------------------- |
| `plugin_session.capability_missing`                                            | 插件没声明这个能力                                                                                   |
| `plugin_session.not_ready`、`plugin_session.plugin_missing`                    | 插件被关掉、取消信任、内容变了或已卸载；面板随即关闭，标签里说明原因。没在调用的面板也会在几秒内关闭 |
| `plugin_session.session_missing`                                               | 会话已不在                                                                                           |
| `plugin_session.message_invalid`、`plugin_session.dedupe_key_invalid`          | 消息或去重键不符合上面的规则                                                                         |
| `plugin_session.notice_invalid`、`plugin_session.status_invalid`               | 提示或状态不符合上面的规则                                                                           |
| `plugin_session.dedupe_key_reused`                                             | 这个去重键已经用在另一条消息上                                                                       |
| `plugin_session.command_unknown`、`plugin_session.command_unusable`            | 插件没有这个命令，或它现在不能用                                                                     |
| `plugins.setting_unknown`、`plugins.setting_secret`、`plugins.setting_invalid` | 清单没声明这个设置、它是保密设置，或值不符合上面的规则                                               |
| `plugins.revision_changed`                                                     | 插件设置刚在别处改过，再试一次                                                                       |
| `plugin_panel.too_many_calls`                                                  | 调用太频繁：每个面板 10 秒内最多 40 次，其中发消息、运行命令和改设置加起来最多 5 次                  |
| `plugin_panel.too_many_notices`                                                | 提示太多：每个插件 10 秒内最多 3 条                                                                  |
| `plugin_panel.invalid`                                                         | 参数的形状不对                                                                                       |
| `plugin_panel.core_unavailable`、`plugin_panel.failed`                         | Aura 这次没有完成，稍后再试                                                                          |

## 查问题

`plugins.logs` 能读到面板的问题：页面的 `console.error`、被拦下的地址、想打开新窗口或跳出面板、加载失败和崩溃（每个面板每分钟最多记 10 条）。面板崩溃或加载失败时，它的标签显示原因和“重新打开”。

## 例子

`panel/index.html`：

```html
<!doctype html>
<html lang="zh-CN">
  <head>
    <meta charset="utf-8" />
    <link rel="stylesheet" href="style.css" />
  </head>
  <body>
    <h1 id="title"></h1>
    <p id="usage"></p>
    <script src="app.js"></script>
  </body>
</html>
```

`panel/app.js`：

```js
const title = document.getElementById('title');
const usage = document.getElementById('usage');

async function refresh() {
  const state = await auraPanel.state();
  title.textContent = state.title;
  usage.textContent = `输入 ${state.usage.input_tokens}，输出 ${state.usage.output_tokens}`;
}

refresh().catch((error) => console.error(error.message));
setInterval(() => refresh().catch((error) => console.error(error.message)), 5000);
```
