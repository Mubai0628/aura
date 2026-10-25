# An example Aura Hook: refuse writes to .env files.
# Aura writes one JSON envelope to stdin and reads one JSON result from stdout:
#   {"decision":"continue"} or {"decision":"block","reason":"..."}.
# Both directions are UTF-8, whatever the console code page is.
[Console]::InputEncoding = [System.Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new($false)
$envelope = [Console]::In.ReadToEnd() | ConvertFrom-Json
$path = [string]$envelope.payload.arguments.path
$name = [System.IO.Path]::GetFileName($path.Replace('/', '\'))
if ($name -eq '.env' -or $name -like '*.env' -or $name -like '.env.*') {
    Write-Output (@{ decision = 'block'; reason = "不允许修改 $name：环境文件里常有密钥，请手动编辑。" } | ConvertTo-Json -Compress)
} else {
    Write-Output '{"decision":"continue"}'
}
