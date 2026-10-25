# 目录结构与编码约定

## 目录

| 目录 | 内容 |
| --- | --- |
| `crates/` | Rust：按领域、契约、执行后端和基础设施划分 |
| `apps/desktop/electron-main` | Electron Main：生命周期、窗口、与 Core 的连接、更新 |
| `apps/desktop/preload` | Renderer 能用的接口，带版本、白名单 |
| `apps/desktop/renderer/src` | 界面（React） |
| `apps/desktop/tests` | 端到端测试 |
| `packages/contracts` | 生成的 TypeScript 契约与校验 |
| `resources/` | Schema、路由表、提示词、内置工具目录、图标 |
| `fixtures/` | 测试夹具 |
| `runtimes/` | 随应用发布的运行时的固定版本（PowerShell） |
| `scripts/` | 构建、架构检查与发布脚本 |
| `xtask/` | `cargo xtask` 任务 |

## 约定

- **依赖方向**：`aura-domain-common` 只放共享基元；`aura-core` 负责组装与协调，不放领域实现；Renderer 不导入 Electron 或 Node API。`check-architecture` 检查这些方向。
- **文件规模**：新建的手写文件不超过 1000 行，目标 500 行；新能力放在独立的模块目录里，`mod.rs` 只做组装。已经超长的文件不能再变长。
- **注释**：只写技术原因与约束，不写计划阶段、日期或人名；来由写在提交说明里。
- **Rust**：Edition 2024，`rustfmt` 与工作区 Clippy 规则；公开 API 显式表达类型、错误与版本。
- **TypeScript**：strict，Prettier 与 ESLint。
- **安全**：先满足权限和强制执行要求再选执行后端，做不到就拒绝，不静默降级；Windows 路径权限按最终路径、句柄与文件身份判断。
- **操作语义**：保留操作身份、权限绑定、尝试、结果与幂等；结果未知的副作用不自动重放。
- **故障体验**：失败时说明原因、影响范围和下一步，保留查看、停止和修复的入口；涉及数据丢失或权限变化时由用户决定，不静默重置。

## 提交

一个提交围绕一项完整改动；改了 Rust 契约，就把重新生成的文件一起提交。不提交凭据、本机缓存或构建输出。
