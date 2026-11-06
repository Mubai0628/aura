# 示例：接本机假聊天平台的插件

一个完整的渠道插件，平台是本机跑的假平台，用来照着写真平台的适配、在接真平台之前把流程跑通。

- `platform/fake_platform.py`：假平台（HTTP 长轮询收消息、`/send` 发消息），不属于插件。
- `plugin/`：插件文件夹。
  - `aura-plugin.toml`：一个渠道（`platform = "other"`）、后台服务、一个文字设置“假平台的地址”。能力：`session.send` 收消息；`session.read` 给 `/sessions`、`/status`；`session.control` 给 `/stop`、`/switch`、`/new`、`/model`、`/permission` 和远程批准。
  - `service/main.py`：适配。长轮询 `/updates`，每条消息交给 `message.in`；`ready` 里报一条消息最多 500 个字符，Aura 的 `reply` 已按它分好段，逐个发到 `/send`。假平台没有按钮，Aura 交来的审批整段发到那个私聊；那个人回的 `/approve` 照常是一条消息。连不上平台时退避着重试，不退出。
  - `service/aura_plugin.py`：从 Skill 的 `sdk/python/` 复制来的帮助库。

## 试一遍

1. 起假平台：`python platform/fake_platform.py --port 8765`。
2. 把 `plugin/` 复制到工作区的 `plugins/fake-chat/`，`plugins.validate`、`plugins.dev_install`。
3. 请用户在 设置 → 插件 里：信任它；“假平台的地址”填 `http://127.0.0.1:8765`；打开后台服务；会话范围选一个工作区；白名单加用户编号 `u1`。要试审批再打开“远程批准”。
4. 替用户发一条：

   ```text
   POST http://127.0.0.1:8765/push
   {"chat_type": "private", "chat_id": "u1", "user_id": "u1", "user_name": "老王", "text": "你好"}
   ```

5. 看结果：Aura 的会话里出现“来自插件 fake-chat · 其他平台 · 老王”的消息，回合结束后 `GET /sent` 里有回复；`plugins.logs` 里有每条消息的回答。
6. 再试：
   - 同一个 `message_id` 推两次（第二次是 `repeated`）、白名单外的 `user_id`（`user_not_allowed`，插件页列出这个人）、群聊（`chat_type` 写 `group`）。
   - `/help`、`/status` 这些命令，回答在 `GET /sent` 里。
   - 审批：推 `/permission 1` 把会话切到只读（往严了切直接生效），再推一条“写一个文件”。`GET /sent` 里出现“Aura 请你决定（编号 1）：……”，3 秒后推 `/approve 1`，回合接着做完。
