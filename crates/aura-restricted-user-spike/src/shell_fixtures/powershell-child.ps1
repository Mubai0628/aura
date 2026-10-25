[IO.File]::WriteAllText('powershell-child.txt', 'child')
@{
    version = $PSVersionTable.PSVersion.ToString()
    runtime = $PSHOME
    modules_fixed = $env:PSModulePath -eq [IO.Path]::Combine($PSHOME, 'Modules')
} | ConvertTo-Json -Compress | Set-Content -LiteralPath 'powershell-child-runtime.json' -Encoding utf8
exit 7
