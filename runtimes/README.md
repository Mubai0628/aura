# Managed runtimes

Aura 随附的 PowerShell 由 [powershell.json](powershell.json) 固定：官方 PowerShell 7.6.5 Windows x64 ZIP 的地址、大小、SHA-256 和逐文件清单摘要。二进制只放在忽略的 `target/managed-runtimes`，不提交缓存或第三方二进制。

准备时先从固定清单里的官方地址下载 ZIP，再执行 `pwsh -NoProfile -File scripts/r06/prepare-runtime.ps1 -Archive <ZIP 的绝对路径>`。脚本核对大小、SHA-256、安全的归档路径、展开限额、逐文件摘要和 Microsoft Authenticode 签名，保留官方包自带的 MIT LICENSE 和 ThirdPartyNotices，自己不下载任何东西。`scripts/release/assemble.mjs` 组装发布时会再按固定清单核对一次。

CMD 使用 Windows 11 自带的系统组件，Aura 不从 PATH 或 COMSPEC 选择 Shell。
