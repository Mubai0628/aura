$ErrorActionPreference = 'Stop'
@{
    version = $PSVersionTable.PSVersion.ToString()
    runtime = $PSHOME
    environment_revision = $env:AURA_EXECUTION_ENVIRONMENT
} | ConvertTo-Json -Compress | Set-Content -LiteralPath 'cmd-child-runtime.json' -Encoding utf8
