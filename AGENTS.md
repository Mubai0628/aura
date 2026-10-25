# Aura 协作规则

给在这个仓库里工作的人和编程智能体。开发文档从 [docs/dev/README.md](docs/dev/README.md) 进入。

## 项目结构

- `crates/`：Rust，按领域、契约、执行后端与基础设施划分。`aura-domain-common` 只放共享基元，`aura-contract-schema` 汇总导出契约，`aura-core` 负责组装与协调，不放领域实现。
- `apps/desktop/electron-main`、`apps/desktop/preload`、`apps/desktop/renderer/src` 分别负责桌面生命周期、受控接口和界面。Renderer 不导入 Electron 或 Node API；Preload 只暴露带版本、经过白名单的接口。
- `packages/contracts` 存放生成的 TypeScript 契约与校验；`scripts`、`xtask` 放可重复的自动化。

## 命令

在仓库根目录用 PowerShell。版本以 `rust-toolchain.toml`、`.node-version`、`package.json` 为准。

| 命令                             | 用途                               |
| -------------------------------- | ---------------------------------- |
| `corepack pnpm install`          | 安装依赖                           |
| `corepack pnpm dev`              | 开发模式启动（Renderer 端口 5175） |
| `cargo xtask generate-contracts` | 改了 Rust 契约后重新生成           |
| `cargo xtask check-fast`         | 提交前的快速检查                   |
| `cargo xtask check-all`          | 完整检查，发布前运行               |

## 编码

- 新建的手写文件不超过 1000 行，目标 500 行；新能力放独立模块目录，`mod.rs` 只做组装；已超长的文件不能再变长。测试按行为拆到 `tests/` 下的多个文件。
- 注释只写技术原因与约束，不写计划阶段、日期或人名。
- Rust 类型是契约的唯一来源；Schema、夹具和 TypeScript 由生成器更新，不手写副本。见 [docs/dev/contracts.md](docs/dev/contracts.md)。
- 安全策略、执行后端能力与就绪状态分开处理；先满足权限和强制执行要求再选后端，做不到就拒绝，不静默降级。Windows 路径权限按最终路径、句柄与文件身份判断。
- 保留操作身份、权限绑定、尝试、结果、幂等与“副作用未知”的语义；拆分模块、更换后端或重新连接都不能丢掉这些约束，也不自动重放结果未知的操作。
- 界面使用真实数据；工程上的重构不改变产品交互。
- 失败时说明原因、影响范围与下一步，保留查看、停止和修复的入口；涉及数据丢失、权限变化或恢复范围不确定时由用户决定，不静默重置，也不连带删除凭据、会话或工作区。

## 测试

- 修缺陷先写能复现它的测试并确认失败，再修实现。
- 只运行受影响的测试和相关检查；`check-all` 在发布前运行。
- 契约改动要覆盖 Rust 序列化、TypeScript 运行时校验、兼容性与确定性；涉及进程、文件、网络和执行的改动要在真实边界上测试，模拟替身不能代替。

## 提交

- 一个提交围绕一项完整改动；Rust 契约与重新生成的文件一起提交。
- 不提交凭据、私人数据、本机缓存、构建输出或未签名的第三方运行时。
