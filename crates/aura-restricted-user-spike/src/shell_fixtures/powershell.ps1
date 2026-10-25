$ErrorActionPreference = 'Stop'
$utf8 = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $utf8
$OutputEncoding = $utf8
$pipeline = (1..3 | ForEach-Object { $_ * 2 }) -join ','
[IO.File]::WriteAllText('中文 空格.txt', '引号 "hello" ''world''', $utf8)
$pipeline | Set-Content -LiteralPath 'powershell-redirect.txt' -NoNewline -Encoding utf8
$childImage = (Get-Command pwsh -CommandType Application -ErrorAction Stop).Source
$childRuntimeMatches = [IO.Path]::GetFullPath($childImage) -eq [IO.Path]::GetFullPath((Join-Path $PSHOME 'pwsh.exe'))
& $childImage -MTA -NoLogo -NoProfile -NonInteractive -File r06-powershell-child.ps1
$childExit = $LASTEXITCODE
@{
    version = $PSVersionTable.PSVersion.ToString()
    edition = $PSVersionTable.PSEdition
    apartment = [Threading.Thread]::CurrentThread.ApartmentState.ToString()
    architecture = [Runtime.InteropServices.RuntimeInformation]::ProcessArchitecture.ToString()
    environment_revision = $env:AURA_EXECUTION_ENVIRONMENT
    child_exit = $childExit
    child_runtime_matches = $childRuntimeMatches
    pipeline = $pipeline
    profile_loaded = [bool](Get-Variable AuraProfileLoaded -ErrorAction SilentlyContinue)
    parent_environment_leaked = [bool]$env:AURA_PARENT_ENV_CANARY
    environment_checks = @{
        modules = $env:PSModulePath -eq (Join-Path $PSHOME 'Modules')
        path = $env:PATH -eq "$PSHOME;$env:SystemRoot\System32;$env:SystemRoot"
        pathext = $env:PATHEXT -eq '.COM;.EXE;.BAT;.CMD'
        comspec = $env:COMSPEC -eq (Join-Path $env:SystemRoot 'System32/cmd.exe')
    }
    environment_paths_valid = ($env:PSModulePath -eq (Join-Path $PSHOME 'Modules')) -and
        ($env:PATH -eq "$PSHOME;$env:SystemRoot\System32;$env:SystemRoot") -and
        ($env:PATHEXT -eq '.COM;.EXE;.BAT;.CMD') -and
        ($env:COMSPEC -eq (Join-Path $env:SystemRoot 'System32/cmd.exe'))
} | ConvertTo-Json -Compress
# A nonzero child exit is an observed result, not an implicit parent failure.
exit 0
