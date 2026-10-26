# Prompt resources

系统提示由这个目录里的模块组成。`prompt-bundle.toml` 是唯一的登记处：每个模块写明 `id`、`version`、`path`、是否必需、所在段落、激活条件和顺序。`crates/aura-prompt-runtime/build.rs` 在编译时读取它，把模块文字和清单哈希嵌进 Core。

## 激活条件

- `always`：每次都下发（`core/` 下的模块）。
- `agent_mode:normal|plan|goal`：按会话当前的任务方式下发（`modes/`）。
- `capability:<名字>`：加载了能产生这个能力名的工具时下发（`capabilities/`）。工具名到能力名的对应在 `crates/aura-core/src/agent_application/context/` 里组装激活输入的地方。
- `compaction`、`maintenance:memory`：压缩和记忆维护按模块 id 直接取用。
- `child_agent`、`recovery:*`、`maintenance:failure_summary`、`maintenance:environment_update`：产品里没有任何路径会触发，在清单里用 `# UNWIRED:` 标出。

## 修改模块

1. 改模块的 Markdown。
2. 在 `prompt-bundle.toml` 里把这个模块的 `version` 加一个版本号，同时改 `bundle_version`。旧数据库里同一 `bundle_version` 的记录带着旧的清单哈希，所以改清单（哪怕只改注释）都要换 `bundle_version`。
3. 运行 `cargo test -p aura-prompt-runtime`，以及组装提示词的相关测试。

`manifest.json` 是最初设计留下的空清单，不登记任何模块；`node scripts/validate-manifests.mjs` 只检查它的格式。
