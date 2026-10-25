# 契约

进程之间、Core 与工具之间交换的数据结构由 Rust 类型定义，它们是唯一的事实来源。不要手写这些类型的 TypeScript 副本或 JSON Schema。

## 生成

```powershell
cargo xtask generate-contracts
```

从 Rust 类型生成：

- `resources/schemas/aura-contracts.schema.json`：全部契约的 JSON Schema；
- `packages/contracts/src/generated/`、`apps/desktop/shared/generated/`：TypeScript 类型与运行时校验；
- `resources/ipc/routes.v1.json` 对应的路由绑定、契约夹具、清单示例和内置工具目录。

`cargo xtask contracts --check` 确认已提交的生成物与重新生成的完全一致。Rust 类型上的文档注释会成为 Schema 里的描述，所以改注释也需要重新生成。

## 版本与兼容

- 每个契约带 `ContractVersion`：主版本必须相同，输入的次版本不能高于实现支持的版本。
- 默认拒绝未知字段。新增字段一般设为可选，旧数据仍能读取。
- ID、修订号和时间的格式见 `aura-domain-common`。

## 契约基线

`scripts/architecture/contract-baseline.json` 记录每个 Schema 定义、IPC 路由、内置 Agent 工具、内置工具定义和 ID 前缀的哈希，`check-architecture` 要求当前代码与它一致。有意修改契约时：

```powershell
cargo xtask generate-contracts
node scripts/architecture/contract-baseline.mjs --write
```

把更新后的基线和改动一起提交，评审时从基线的差异就能看到哪些契约变了。删除条目会让仍在使用它的一方读不了数据，需要另加 `--allow-removal`。

`scripts/architecture/contract-invariants.test.mjs` 检查不随版本变化的性质，例如密钥只走密钥输入路由、剪贴板与外部链接只在 Main、设置类操作不提供给模型。改动让它失败时，先确认是否真的要改变这些性质。

## 内置工具

内置工具的定义（名称、说明、输入 Schema）是发给模型的契约，已发布的版本不可改写。修改一个已发布工具的定义要作为它的下一个修订发布，包括它输入类型上的文档注释。
