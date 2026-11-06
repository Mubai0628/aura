# 插件的各部分

```
<插件>/
  aura-plugin.toml
  skills/<名字>/SKILL.md         Skill
  commands/<名字>/command.toml   提示命令
  commands/<名字>/template.md
  hooks/<名字>/hook.toml          Hook
  hooks/<名字>/check.ps1
  mcp.json                        MCP 服务器
  panel/index.html                面板（清单里写 [panel]）
  bin/...                         程序
```

## Skill：`skills/<名字>/SKILL.md`

开头是 YAML front matter，后面是给模型看的 Markdown：

```markdown
---
name: review-guide
description: 这个团队怎样审查改动；审查代码时用
---

先读完整个改动再提意见……
```

- `name`：小写字母、数字和 `-`，最多 64 个字符。
- `description`：最多 1024 个字符，写清楚什么时候用。
- 同一个文件夹里可以放它要读的文件，用相对链接指向它们。

## 提示命令：`commands/<名字>/`

用户在输入框打 `/<名字>` 时，把模板当作消息发出。

`command.toml`：

```toml
name = "review"              # 和文件夹名相同；小写字母、数字和 -，最多 40 个字符
description = "审查改动"      # 必填，一行，最多 200 个字符
title = "审查"               # 可选，最多 40 个字符
argument_hint = "<范围>"     # 可选，最多 80 个字符
version = 1                  # 正整数
```

`template.md`：消息正文，最多 32 KB；`$ARGUMENTS` 处放用户在命令后面打的字。只能链接命令文件夹里的文件，不能含凭据。命令只是一条消息，不带任何权限，也不运行程序。

## Hook：`hooks/<名字>/hook.toml`

在会话的固定时刻运行插件里的一个程序。程序从标准输入读一个 JSON，往标准输出写一个 JSON 结果。

```toml
schema_version = 1
name = "format-check"           # 小写字母、数字和 -，最多 64 个字符
version = "1.0.0"
description = "写文件前检查格式"
kind = "user"                   # user（失败当作继续）或 security（失败即阻断）
timeout_ms = 10000              # 100 到 60000
events = ["tool.before_call"]

[command]
powershell_script = "check.ps1" # 或 executable = "check.exe"；必须是这个 Hook 文件夹里的相对路径
arguments = []

[match]
tools = ["fs.write", "shell.*"] # 只对 tool.* 事件有意义；不写就是全部工具
```

事件：`session.started`、`session.resumed`、`message.before_accept`、`turn.before_start`、`context.before_build`、`model.before_call`、`model.after_call`、`tool.before_call`、`tool.after_call`、`tool.on_error`、`context.before_compact`、`context.after_compact`、`turn.before_complete`、`turn.completed`。

程序读到的 JSON：`{"protocol": "aura.hook.v1", "event": "...", "hook": {"name": "...", "version": "..."}, "session_id": "...", "turn_task_id": "...", "workspace_root": "...", "payload": {...}}`。

程序写出的结果只能是其中一种：

- `{"decision": "continue"}`
- `{"decision": "block", "reason": "原因"}`
- `{"decision": "transform", "<字段>": ...}`：只有部分事件能变换，每个事件只能带它自己的那个字段（如 `text`、`inject`、`arguments`、`result`、`error`、`focus`、`parameters`）。

结果里有别的字段，或程序超时、退出码不是 0，都算失败：`user` 当作继续，`security` 阻断。失败会记进插件的日志，用 `plugins.logs` 读。

## MCP 服务器：`mcp.json`

```json
{
  "servers": [
    {
      "name": "files",
      "display_name": "文件",
      "transport": {
        "type": "stdio",
        "command": "${PLUGIN_ROOT}/bin/server.exe",
        "arguments": ["--root", "${PLUGIN_ROOT}"],
        "environment": [{ "name": "LOG_LEVEL", "value": "info" }],
        "secret_environment": ["GITHUB_TOKEN"]
      }
    },
    {
      "name": "docs",
      "transport": {
        "type": "http",
        "url": "https://mcp.example.com/mcp",
        "auth": { "type": "oauth", "scopes": ["read"] }
      }
    }
  ]
}
```

- 最多 8 个服务器。每个以 MCP 服务器 `<插件名>-<name>` 运行。
- `${PLUGIN_ROOT}` 换成插件文件夹；其他 `${...}` 不会展开。
- 密钥写在 `secret_environment`（环境变量名）或 `auth` 里（`{"type": "secret"}` 默认用 `Authorization: Bearer`，也可以写 `"header"`；或 `{"type": "oauth"}`），值由用户在 Aura 里填。
- 服务器的标准错误最近几十行会进插件的日志。
