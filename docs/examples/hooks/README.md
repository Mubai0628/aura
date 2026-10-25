# Hook 示例

L07 的 Hook 是你自己的程序：一个文件夹，根目录放 `hook.toml`，程序或脚本放在包内。Aura 在会话的固定时刻启动它，标准输入给一个 JSON，标准输出读一个 JSON 结果。完整说明见设置 → Hook 里的“Hook 包格式”。

## protect-env

阻止模型写入或修改 `.env` 文件（`fs.write`、`fs.patch` 调用前检查目标文件名）。

1. 打开 设置 → Hook，点“添加 Hook…”，选择 `docs/examples/hooks/protect-env` 文件夹。
2. 预览里核对：运行“用 Aura 自带的 PowerShell 运行包内脚本 protect-env.ps1”，事件“工具调用前”，工具 `fs.write`、`fs.patch`。点“信任并启用”。
3. 在会话里让模型新建或修改工作区里的 `.env`：调用不执行，模型收到“被 Hook protect-env 阻止：…”，轮次结束显示 Hook 统计。

改动这个文件夹里的任何文件之后，它不会再运行，设置页提示“内容已变化，需要重新信任”。
