# Aura 开发文档

- [架构概览](architecture.md)
- [构建与测试](building-and-testing.md)
- [契约](contracts.md)
- [发布流程](releasing.md)
- [目录结构与编码约定](conventions.md)

## 快速开始

需要：

- Windows 11 x64；
- Visual Studio 2022 生成工具（“使用 C++ 的桌面开发”，提供 MSVC 与 Windows SDK）；
- Rust：`rustup` 会按 `rust-toolchain.toml` 自动安装固定版本（当前 1.97.1）；
- Node.js 24.18.0（见 `.node-version`），pnpm 通过 Corepack 使用 `package.json` 固定的版本；
- Git for Windows 2.40 或更新（Aura 的 Git 功能和部分测试用到）。

在仓库根目录（PowerShell）：

```powershell
corepack pnpm install
corepack pnpm --filter @aura/desktop exec node-gyp install --ensure
corepack pnpm dev
```

第二条把 Node 的头文件装进 `%LOCALAPPDATA%\node-gyp\Cache`（已有就跳过），Main 与 Core 之间的命名管道原生模块用它编译；也可以让 `AURA_NODE_API_SDK` 指向含 `include/node` 与 `x64/node.lib` 的文件夹。`pnpm dev` 构建 Core 和相关程序，启动 Renderer 开发服务器（端口 5175）和 Electron 窗口。开发版的数据与安装版分开存放。

提交前至少运行：

```powershell
cargo xtask check-fast
```

完整检查见[构建与测试](building-and-testing.md)。
