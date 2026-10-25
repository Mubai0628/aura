---
name: answer-signature
description: 按固定格式回答并在末尾署名。用户提到“署名回答”或 answer-signature 时使用。
allowed-tools: fs.write
---

# 署名回答

1. 先用 `skills.read_resource` 读取[格式清单](references/checklist.md)，按清单组织回答。
2. 回答的最后单独一行写：`—— 按 Skill answer-signature 回答`。
3. 用户要求保存时，把回答写入工作区根目录的 `answer.md`。写文件照常按会话的权限模式和批准执行，这个 Skill 不能替用户批准。
