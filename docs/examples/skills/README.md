# Skill 示例

L09 的 Skill 是一个文件夹：根目录的 `SKILL.md` 开头是 YAML（`name`、`description`，可选 `allowed-tools` 等），正文是写给模型的说明；其余文件是模型按需读取的资料。Skill 只是说明：不能运行程序，也不给任何权限，它提到的工具照常按会话的权限模式和批准执行。

## answer-signature

让模型按格式清单回答，并在末尾署名；你要求保存时写入工作区的 `answer.md`（照常按权限模式询问）。

1. 打开 设置 → 扩展 → Skill，点“导入文件夹…”，选择 `docs/examples/skills/answer-signature`。Aura 把它复制到自己的 Skill 库，原文件夹不变。
2. 在列表里点“查看并信任”：核对 `SKILL.md` 原文、两个文件，以及“它说需要的工具：fs.write（只是说明，不会因此允许任何工具）”。点“信任并启用”。
3. 在会话里说“用 answer-signature 署名回答：……”。模型用 `skills.search` 找到它、`skills.load` 加载、`skills.read_resource` 读格式清单，在任何权限模式下都不询问；回答最后一行是署名。

导入后的副本有任何改动（点“打开文件夹”可以找到它），都要你重新信任，模型才能再用它。
