# 架构概览

## 进程

```text
AuraLauncher.exe          选择要运行的版本，核对组件，处理试运行与恢复窗口
  └─ Aura.exe (Electron Main)
       ├─ Preload / Renderer   界面；Renderer 不接触 Electron 或 Node API
       └─ aura-core.exe        领域服务、模型调用、工具执行、存储
            ├─ aura-egress-proxy.exe     按需启动，联网工具的出口代理
            ├─ aura-sandbox-host.exe     按需启动的 AppContainer 执行宿主；当前没有功能把执行交给它
            └─ aura-elevated-helper.exe  只在需要管理员权限的修复时启动
```

- **Main 与 Core** 通过命名管道通信（`aura-main-pipe.node`）。所有请求走登记在 `resources/ipc/routes.v1.json` 的路由：Main 面向 Renderer 的路由以可信 Renderer 身份调用，Core 的路由只接受经过认证的 Main；密钥只走专门的密钥输入路由。
- **Preload** 只暴露带版本、经过白名单的接口（`window.aura.v1`）。
- **Core** 是唯一写入数据的进程，只有一个登记过的例外：`aura-sandbox-host` 运行时自己打开数据库（见 `scripts/architecture/legacy-exceptions.json`，计划把这部分存储访问移进 Core）。`aura-egress-proxy` 只链接了存储 crate，不打开数据库。
- 模型的每次工具调用先经过权限引擎（模式、审批、网站与文件范围），再选择执行后端；不满足权限或强制要求时拒绝，不静默降级。
- **执行后端的现状**：
  - 模型的 PowerShell、CMD、终端和工具包程序走兼容后端，以用户身份在 Job 里运行，类别如实声明为 Native，不在沙箱里（S08 时所有者选定兼容优先）。
  - AppContainer 执行（`aura-sandbox`、`aura-sandbox-host`）随正式版发布，但当前没有工具把执行交给它，原来的工具起草与沙箱执行路径已停用。
  - 受限账户执行链（`aura-command-runner`、`aura-execution-helper`、`aura-execution-maintainer`）只在开发环境构建，不在正式版的发布布局里。
  - 这些未接通的代码都用 `UNWIRED:` 注释标出（`git grep -n "UNWIRED:"`）。

## 主要 crate

| 范围 | crate |
| --- | --- |
| 共享基元与契约 | `aura-domain-common`、`aura-runtime-contract`、`aura-security-contract`、`aura-execution-contract`、`aura-tool-contract`、`aura-contract-schema`（汇总导出） |
| Core 组装与应用协调 | `aura-core` |
| 会话与模型 | `aura-agent-domain`、`aura-context-*`、`aura-model-*`、`aura-prompt-runtime` |
| 权限 | `aura-permission-domain`、`aura-permission-engine`、`aura-approval` |
| 工具 | `aura-tool-*`、`aura-fs-tools`、`aura-net-tools`、`aura-git-service`、`aura-web-*`、`aura-browser-domain` |
| 执行 | `aura-backend-*`、`aura-windows-platform`、`aura-sandbox*`、`aura-egress-proxy`、`aura-network-control` |
| 扩展 | `aura-mcp`、`aura-skills`、`aura-plugins`、`aura-hooks`、`aura-commands` |
| 存储 | `aura-persistence`、`aura-persistence-core`、`aura-persistence-v2`、`aura-memory*`、`aura-config` |
| 安装与启动 | `aura-launcher`、`aura-setup` |

名字里带 `p03`–`p08`、`r06` 的模块和功能（例如 `p04.rs`、`p08-contract-driver`）是按最初设计划分的子系统名，沿用至今。

## 数据

安装版的数据在 `%LOCALAPPDATA%\Aura\namespaces\<安装命名空间的哈希>`：SQLite 数据库、设置文件、日志、启动器状态与更新前的数据备份。密钥在 Windows 凭据管理器。开发版使用自己的命名空间，不会和安装版混在一起。

## 安装布局与更新

```text
<安装目录>\AuraLauncher.exe          版本目录里启动器的副本
<安装目录>\Uninstall Aura.exe        版本目录里安装程序的副本
<安装目录>\active.json               当前版本、上一个版本、等待试运行的版本
<安装目录>\versions\<版本>\           一个完整的版本，含 component-manifest.json 与签名
```

正式版的组件清单由发布根证书签名，安装与启动都核对清单和每个组件的哈希。新版本先作为“等待中”的版本试运行，Main 在窗口显示、Core 就绪后报告成功，启动器才把它设为当前版本；试运行失败则回到原版本并恢复更新前的数据。启动器在新版本第一次启动前运行新版本的 `aura-core.exe --store-migration-check <数据目录>`，只复制它回答会迁移（或回答不了）的数据库及其日志文件到 `backups\before-<版本>`；都不迁移时不复制，并删掉更早的副本。磁盘空间不够（需要副本大小再加 10%）或复制失败时，由用户选择退出或不备份启动。
