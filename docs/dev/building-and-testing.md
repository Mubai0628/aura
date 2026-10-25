# 构建与测试

所有命令在仓库根目录的 PowerShell 里运行。Cargo 默认 6 个并行任务（`.cargo/config.toml`），开发与测试构建用 `rust-lld` 链接。

## 常用命令

| 命令 | 用途 |
| --- | --- |
| `corepack pnpm install` | 安装 JavaScript 依赖 |
| `corepack pnpm dev` | 构建并以开发模式启动 |
| `cargo test -p <crate>` | 一个 crate 的测试；终端、兼容执行等测试只在打开对应功能时编入，`--all-features` 全部打开 |
| `cargo xtask test-isolated <cargo test 的参数>` | 带一份临时的独立 Git 配置运行 `cargo test`。打开 Git 测试功能（`--all-features`）后，Git 相关测试不读你自己的 Git 配置，必须这样运行，例如 `cargo xtask test-isolated -p aura-core --lib --all-features -- l03_git` |
| `corepack pnpm test` | 桌面端与契约包的单元测试（Vitest）；其中几项走真实的命名管道，要先编出原生模块（`corepack pnpm --filter @aura/desktop build:native`，`pnpm dev` 与 `pnpm build` 也会编） |
| `corepack pnpm test:e2e` | 桌面端端到端测试（Playwright + Electron），会先构建 Core 与桌面端 |
| `cargo xtask check-architecture` | 文件规模、依赖方向、Renderer 边界、契约基线与契约不变量、ESLint |
| `cargo xtask check-fast` | 上一项，加 Rust 格式、契约确定性、资源清单与 TypeScript 类型检查 |
| `cargo xtask check-all` | 完整检查：上面全部，加 Clippy、全部 Rust 测试（`--all-features`）、Prettier、构建与端到端测试；`--skip-e2e` 跳过端到端 |

## 测试的写法

- 修缺陷先写能复现它的测试，确认失败后再修。
- 测试按行为拆到 `tests/` 下的多个文件；新文件不超过 1000 行。
- 涉及真实边界（进程、文件、网络、执行）的改动，要有走真实边界的测试；模拟替身不能代替。
- 端到端测试的数据放在临时目录，结束后删除。

## 只运行受影响的部分

日常改动只跑受影响 crate 的测试和相关检查；`check-all` 在发布前运行。
