# 构建与测试

所有命令在仓库根目录的 PowerShell 里运行。Cargo 默认 6 个并行任务（`.cargo/config.toml`），开发与测试构建用 `rust-lld` 链接。

## 常用命令

| 命令 | 用途 |
| --- | --- |
| `corepack pnpm install` | 安装 JavaScript 依赖 |
| `corepack pnpm dev` | 构建并以开发模式启动 |
| `cargo xtask test [-p <crate>]… [--lib\|--bins\|--tests\|--test <名字>] [-- <过滤与测试参数>]` | Rust 测试。先用 `check-all` 的编译配置（`cargo test --workspace --all-features --no-run`）编出全部测试程序，再只跑选中的：按 cargo 的方式在包目录里逐个运行，一个失败了也跑完其余的再汇总。终端、兼容执行、Git 等测试都编进去；Git 测试读本轮临时的独立 Git 配置，不读你自己的。例：`cargo xtask test -p aura-core --lib -- l03_git`、`cargo xtask test -p aura-fs-tools --tests -- p03_fs_tools::`。`test-isolated` 是它的旧名字 |
| `cargo xtask clippy` | `cargo clippy --workspace --all-features --all-targets -- -D warnings`，和 `check-all` 同一份检查产物 |
| `cargo xtask sweep-target [--apply] [--idle-days <n> \| --idle-hours <n>] [--unused]` | 列出 cargo 不会再用的构建缓存，`--apply` 才删，见下文 |
| `corepack pnpm test` | 桌面端与契约包的单元测试（Vitest）；其中几项走真实的命名管道，要先编出原生模块（`corepack pnpm --filter @aura/desktop build:native`，`pnpm dev` 与 `pnpm build` 也会编） |
| `corepack pnpm test:e2e` | 桌面端端到端测试（Playwright + Electron），会先构建 Core 与桌面端。Chrome 控制的测试（`l14-chrome.spec.ts`）用 Playwright 的 Chromium（Chrome for Testing，`npx playwright install chromium`）加载为这个检出的开发命名空间构建的测试扩展，在 HKCU 写这个命名空间的主机注册、在凭据管理器写配对密钥，结束时都删除；`AURA_E2E_CHROME_HEADED=1` 时显示 Chrome 窗口 |
| `cargo xtask check-architecture` | 文件规模、依赖方向、给桌面的失败文字、Renderer 边界、契约基线与契约不变量、ESLint |
| `cargo xtask check-fast` | 上一项，加 Rust 格式、契约确定性、资源清单与 TypeScript 类型检查 |
| `cargo xtask check-all` | 完整检查：上面全部，加 Clippy、全部 Rust 测试（`--all-features`）、Prettier、构建与端到端测试；`--skip-e2e` 跳过端到端 |

## 测试的写法

- 修缺陷先写能复现它的测试，确认失败后再修。
- 集成测试按行为拆到 `tests/<名字>/` 下的多个文件，一个 crate 只有这一个测试程序（见下文）；新文件不超过 1000 行。
- 涉及真实边界（进程、文件、网络、执行）的改动，要有走真实边界的测试；模拟替身不能代替。
- 断言路径长度的测试按实际的临时路径算：`cargo xtask test` 在 `%TEMP%\aura-tests-<进程号>` 里跑，比平时的 TEMP 长十几个字符，用户名长的机器也一样。
- 端到端测试的数据放在临时目录，结束后删除。

## 只运行受影响的部分

日常改动只跑受影响 crate 的测试和相关检查；`check-all` 在发布前运行。

## Rust 构建产物的复用与磁盘

cargo 给每个编译单元起名时，把包、feature、配置（调试信息级别、是否增量编译）、`RUSTFLAGS`、工具链版本和全部依赖的名字一起算进去；一个 crate 拿到哪些 feature，又随命令选了哪些包而变。名字不同就另存一份，旧的 cargo 从不删。2026-11-08 改之前，在主检出量到：

- `target/debug/deps` 266 GiB：`check-all` 的测试构建 94 GiB（571 个测试程序，程序和调试文件 88.6 GiB），别的配置留下的 172 GiB；增量编译状态另有 41 GiB。
- `cargo test -p a` 和 `cargo test --workspace --all-features` 各编一整份，换一组 `-p` 再编一份；临时设 `CARGO_INCREMENTAL=0` 或 `CARGO_PROFILE_DEV_DEBUG=…` 又是一份。同一个测试程序在 `deps` 里最多存了 10 份。
- 每个 `tests/*.rs` 单独链接成一个程序，带着整个 crate 和全部依赖；工作区有 446 个这样的程序，调试文件一个一两百 MB。
- Core 的测试根目录在测试结束时还开着（存储、日志、子进程），`tempfile` 删不掉也不报错：一轮 `check-all` 在 TEMP 里留下 2,248 个目录、34.9 GiB。

现在的做法：

- `cargo xtask test` 和 `cargo xtask clippy` 只用 `check-all` 那一套配置（见上表）。编完后按 cargo 的方式在包目录里逐个运行选中的程序：设好 `CARGO_MANIFEST_DIR` 等变量，程序所在的目录排在 `PATH` 最前，一个失败了也跑完其余的再汇总。`--features`、`--release`、`--target-dir` 这类会换配置的参数不接受。文档测试不跑（工作区只有一个，标了 `ignore`），示例代码写成单元测试。
- 每个 crate 一个集成测试程序 `tests/<名字>/main.rs`，名字是 crate 名去掉 `aura-`（`aura-core` 的叫 `core_tests`）。单跑一个文件的测试写 `--tests -- <文件名>::`。几个文件共用的辅助代码在 `main.rs` 声明一次。`check-architecture` 拒绝一个 crate 有两个以上的集成测试程序。
- 同一程序里不同文件的测试并行运行。`aura-desktop-control` 的测试操作同一个桌面，登记在 `xtask/src/rust_tests/threads.rs`，一次只跑一个。
- 有时间上限的测试（Core 的 PowerShell 返回时间等）在其余测试跑完后单独跑，上限不变；表在 `xtask/src/rust_tests/alone.rs`。和其他测试一起跑时，量到的是并行负载，不是代码。
- Core 的库测试会在本机局域网地址上起 HTTP 服务，Windows 防火墙按程序路径询问，程序名里的哈希一变就再问一次。runner 先把这个程序硬链接成 `deps/aura_core-tests.exe` 再运行，路径不变，同一个 `target` 只问一次。
- 测试在 `%TEMP%\aura-tests-<进程号>` 里跑（`TEMP`、`TMP` 指向它，Git 测试的独立配置也在里面），整轮结束后整个删掉。构建盘少于 60 GB 或 TEMP 盘少于 20 GB 时不开始。
- 工作区代码只带行号调试信息，第三方库不带（`Cargo.toml` 的 `[profile.dev]`）；backtrace 仍有文件和行号，调试器看不到局部变量。
- 每次成功的测试构建和 Clippy 把用到的单元记在 `target/debug/xtask-used/`。`cargo xtask sweep-target` 列出 cargo 不会再用的构建缓存：源码比产物新的单元、每个单元最新一次之外的增量会话、空闲期里没写过的增量目录和测试程序（默认 3 天，`--idle-days` 或 `--idle-hours` 改）；`target/debug` 里还有硬链接的程序（`pnpm dev`、E2E 启动的）不列。`--apply` 才删，有别的 cargo 或 rustc 在跑时不删。
- 换工具链或改 `[profile.dev]` 后，旧配置的单元全部作废，可 cargo 不会改写仍然有效的库，单看时间认不出哪些作废了。先跑一次 `cargo xtask test` 和 `cargo xtask clippy`，再用 `sweep-target --unused`：它加列这两者和 xtask 自己都没用到、最后写入早于这两次运行的单元（看 `target/debug/xtask-used/` 两份记录中较早那份的时间，不看空闲期）。`pnpm dev` 与 E2E 的单元也在其中，下次用到时重编。增量目录的名字看不出属于哪套配置，旧配置的要等空闲期（默认 3 天）；想马上删，在这两次运行之后加 `--idle-hours <n>`，n 取到这两次运行开始之前，再短就连当前配置的增量目录和测试程序一起删了（下次用到时重编、重新链接）。
- `pnpm dev` 的构建带自己的 `RUSTFLAGS`（静态 CRT），E2E 先后按两组包和 feature 构建（第二组打开只给测试用的 `p08-contract-driver`），各是一份。Windows MSVC 下路径包的程序名不带哈希：E2E 的 `aura-core.exe`、`aura-tool-fixtures` 的程序和 `aura-chrome-host` 与测试构建写同一个文件，每换一次就重新链接，所以跑 E2E 规格前总要先让 E2E 的构建跑完。
- 会话工作树构建进自己的 `target`（各检出的同名单元会互相覆盖，不能共用主检出的）；分支合进来后，同一步删掉这个工作树的 `target`。

改完后（2026-11-08，主检出）：

- 测试程序 571 个变成 192 个，程序和调试文件 88.6 GiB 变成 7.6 GiB，最大的是 Core 的库测试（977 MiB）；测试构建在 `deps` 里 94.2 GiB 变成 10.9 GiB。
- 改配置后跑过一次 `cargo xtask clippy` 和 `cargo xtask test`，`sweep-target --unused --apply` 删了 266.4 GiB（源码已变的旧单元 194.0、两份记录都没用到的 66.9、被取代的增量会话 5.5），`--idle-hours 1 --apply` 又删了改配置前的增量目录 35.9 GiB。`target/debug` 剩约 36 GiB（`deps` 15.0、增量 17.1）。之后改一个 crate 只重编它和依赖它的：改完 xtask 再跑 `cargo xtask test -p xtask -p aura-data-move` 用 13 秒，`cargo xtask clippy` 2 秒。
- 全部 Rust 测试一轮 31 分钟，其中改配置后 106 个工作区包全部重编用了 4 分 35 秒；各程序里测试本身的时间合计 1,114 秒，改前 1,140 秒。改前跑的测试改后都还在跑：除 15 个新加的 xtask 测试外一一对上，名字前面多了文件名那一层模块（`store_slimming` 的 22 个原来就有这一层，名字不变）。
