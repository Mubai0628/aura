# 发布流程

## 发布密钥

正式版的组件清单由两级证书签名：根证书只用来签发发布签名证书，根证书的公钥编进程序（`resources/release-trust/production/`）。

```powershell
node scripts/release/release-key.mjs init --backup <移动存储上的文件夹>
```

生成根证书与发布签名证书，私钥加密保存，口令由你输入；根证书的加密备份写到 `--backup`，本机不留根私钥。发布签名证书保存在 `%APPDATA%\Aura Release\signing`。

## 构建

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static -C link-arg=/Brepro"
$env:AURA_PRODUCTION_RELEASE_BUILD = "1"
node scripts/cargo-release.mjs build --release --locked -p aura-core -p aura-launcher -p aura-sandbox-host -p aura-egress-proxy -p aura-elevated-helper -p aura-setup -p aura-chrome-host
corepack pnpm build
corepack pnpm --filter @aura/desktop package:dir
node scripts/release/build-release.mjs --version <x.y.z>
```

`AURA_PRODUCTION_RELEASE_BUILD=1` 把 `resources/release-trust/production/` 下的根证书编进检查签名的程序；正式版没有它时 `build-release.mjs` 直接拒绝。`build-release.mjs` 组装版本目录（桌面包、各程序、Chrome 扩展、固定版本的 PowerShell、许可证与第三方声明），签名组件清单（询问发布签名证书的口令），打出安装程序 `Aura-Setup-<版本>-x64.exe`、更新包 `Aura-Update-<版本>-x64.aura-update` 和 `SHA256SUMS`。

Chrome 扩展放在版本目录的 `resources/chrome-extension`。`corepack pnpm build` 构建的是正式版扩展（`apps/chrome-extension/dist/production`）；组装时核对它的公钥、连接的主机名和版本号都属于这次发布，不符就拒绝并给出构建命令。扩展的版本号取自 `apps/chrome-extension/package.json`，随产品版本一起改。

测试时可以用 `--namespace test:<名字>`，安装到独立的位置，不影响正式安装。测试命名空间配对的是测试扩展，组装前先为它构建：

```powershell
$env:AURA_CHROME_TARGET = "test"; $env:AURA_CHROME_NAMESPACE = "aura.desktop.test.<名字>"
corepack pnpm --filter @aura/chrome-extension build
```

## 公开仓库

开发仓库与公开仓库分开。每次发布从要发布的提交导出公开源码：

```powershell
node scripts/release/export-public.mjs --out <新文件夹> --build
```

导出按 `scripts/release/public-tree.mjs` 选择文件，把 `public/` 下的文件放到对应位置，并检查：密钥（Aura 自己的扫描器）、本机路径与个人信息、超大文件、注释里的过程记录、指向未公开文件的引用、指向不存在文件的导入；`--build` 再把导出的目录当作新仓库安装、编译、跑检查和单元测试。任何一项不通过就不导出。

## 发布

1. 在开发仓库运行 `cargo xtask check-all`。
2. 构建并安装测试，确认更新、回退、卸载正常。
3. 导出公开源码，作为一次提交推送到公开仓库，打上版本标签。
4. 在 GitHub 建 Release，上传安装程序、更新包和 `SHA256SUMS`；说明取自 `CHANGELOG.md` 里这个版本的一节，包括已知限制。

应用内的更新检查读取公开仓库的 Releases，找名为 `Aura-Update-<版本>-x64.aura-update` 的资产。
