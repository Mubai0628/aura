param(
    [ValidatePattern('^aura-[a-z0-9-]+$')][string]$Package = 'aura-desktop-control',
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-zA-Z0-9_]+$')][string]$Test,
    [Parameter(Mandatory=$true)][ValidatePattern('^[a-zA-Z0-9_:]+$')][string]$Name,
    [Parameter(Mandatory=$true)][string]$Evidence,
    [string]$ManifestPath,
    [switch]$Library
)
$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'desktop-idle.ps1')
. (Join-Path $PSScriptRoot 'owner-window.ps1')
. (Join-Path $PSScriptRoot 'stop-test-control.ps1')
$repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
$evidenceRoot = [IO.Path]::GetFullPath($Evidence)
if (-not $evidenceRoot.StartsWith((Join-Path $repo '.aura-dev/acceptance-runs/'), [StringComparison]::OrdinalIgnoreCase)) { throw 'Evidence must be in the acceptance-runs directory' }
New-Item -ItemType Directory -Path $evidenceRoot -Force | Out-Null
$lastPath = Join-Path $evidenceRoot 'last-own-input.json'
if (-not $env:AURA_L18_OWNER_WINDOW -or -not (Test-Path -LiteralPath $env:AURA_L18_OWNER_WINDOW)) { Write-Output 'Blocked: a recorded owner departure and continuous input observer are required'; exit 3 }
if (-not $ManifestPath) { $ManifestPath = Join-Path $repo 'Cargo.toml' }
$ManifestPath = [IO.Path]::GetFullPath($ManifestPath)
if (-not (Test-Path -LiteralPath $ManifestPath -PathType Leaf) -or [IO.Path]::GetFileName($ManifestPath) -ne 'Cargo.toml') { throw 'Invalid manifest' }
$fileName = ($Name -split '::')[-1]
$prefix = $Test + '-' + $fileName + '-' + [DateTime]::UtcNow.ToString('yyyyMMdd-HHmmss-fff')
$statePath = Join-Path $evidenceRoot ($prefix + '-idle.json')
$targetArgument = if ($Library) { '--lib' } else { "--test $Test" }

function Assert-Idle {
    $observed = Read-AuraOwnerWindow -Path $env:AURA_L18_OWNER_WINDOW
    for ($flush = 0; -not $observed.condition.eligible -and -not $observed.condition.revoked -and $flush -lt 10; $flush++) {
        Start-Sleep -Milliseconds 100
        $observed = Read-AuraOwnerWindow -Path $env:AURA_L18_OWNER_WINDOW
    }
    @{ atUtc=[DateTimeOffset]::UtcNow.ToString('o'); systemIdleMs=$observed.idle.idleMs; unlocked=$observed.idle.unlocked; lastInputTick=$observed.idle.lastInputTick; ownerLastUtc=$observed.ledger.ownerLastUtc; condition=$observed.condition; command="cargo --offline test --manifest-path $ManifestPath -p $Package $targetArgument" } | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $statePath
    if (-not $observed.condition.eligible) { Write-Output "Blocked: owner idle window is not available. $statePath"; exit 3 }
    return ([DateTimeOffset]$observed.ledger.ownerLastUtc).ToUniversalTime()
}

$ownerLast = Assert-Idle
[Diagnostics.Process]::GetCurrentProcess().PriorityClass = 'BelowNormal'
$env:CARGO_BUILD_JOBS = '6'
$env:RUST_TEST_THREADS = '6'
$env:CARGO_NET_OFFLINE = 'true'
if ((Get-PSDrive C).Free -lt 80GB) { throw 'C: free space is below 80 GiB' }
if (Get-Process cargo -ErrorAction SilentlyContinue) { throw 'Another cargo invocation is running' }
$buildLog = Join-Path $evidenceRoot ($prefix + '-build.jsonl')
$cargoArguments = @('--offline','test','--manifest-path',$ManifestPath,'-p',$Package)
if ($Library) { $cargoArguments += '--lib' } else { $cargoArguments += @('--test',$Test) }
if ([IO.Path]::GetDirectoryName($ManifestPath) -ne $repo) { $cargoArguments += @('--target-dir',(Join-Path ([IO.Path]::GetDirectoryName($ManifestPath)) 'target')) }
$cargoArguments += @('--no-run','--message-format=json')
& cargo @cargoArguments 1> $buildLog 2> ($buildLog + '.stderr')
if ($LASTEXITCODE -ne 0) { throw "Test compilation failed: $buildLog" }
$exe = Get-Content -LiteralPath $buildLog | ForEach-Object { try { $value = $_ | ConvertFrom-Json; if ($value.reason -eq 'compiler-artifact' -and $value.target.name -eq $Test -and $value.executable) { $value.executable } } catch {} } | Select-Object -Last 1
if (-not $exe) { throw 'Cargo did not report a test executable' }
$ownerLast = Assert-Idle
$stdout = Join-Path $evidenceRoot ($prefix + '.stdout.log')
$stderr = Join-Path $evidenceRoot ($prefix + '.stderr.log')
$tempBase = [IO.Path]::GetFullPath([IO.Path]::GetTempPath()).TrimEnd('\') + '\'
$before = @(Get-ChildItem -LiteralPath $tempBase -Directory -Filter 'aura-desktop-test-*' | ForEach-Object { $_.FullName })
$process = Start-Process -FilePath $exe -ArgumentList @('--exact', $Name, '--ignored', '--test-threads=1', '--nocapture') -WindowStyle Hidden -PassThru -RedirectStandardOutput $stdout -RedirectStandardError $stderr
$interrupted = $false
while (-not $process.WaitForExit(100)) {
    try { $observation = Read-AuraOwnerWindow -Path $env:AURA_L18_OWNER_WINDOW -During }
    catch {
        $interrupted=$true
        Stop-AuraTestControl -TestProcessId $process.Id
        if (-not $process.WaitForExit(10000)) { throw 'Observer failed and the stopped test has not exited; do not start another test' }
        break
    }
    if (-not $observation.condition.eligible) {
        $interrupted = $true
        Stop-AuraTestControl -TestProcessId $process.Id
        if (-not $process.WaitForExit(10000)) { throw 'Control was stopped, but the test has not exited; do not start another test' }
        break
    }
}
$code = $process.ExitCode
if ($null -eq $code) { throw 'No test exit code was available' }
foreach ($folder in (Get-ChildItem -LiteralPath $tempBase -Directory -Filter ("aura-desktop-test-$($process.Id)-*"))) {
    $target = [IO.Path]::GetFullPath($folder.FullName)
    if ($before -contains $target) { continue }
    if (-not $target.StartsWith($tempBase, [StringComparison]::OrdinalIgnoreCase)) { throw 'Fixture cleanup escaped TEMP' }
    if ($folder.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw 'Fixture TEMP must not be a reparse point' }
    Remove-Item -LiteralPath $target -Recurse -Force
    if (Test-Path -LiteralPath $target) { throw 'Fixture cleanup did not finish' }
}
$ticks = @(Get-Content -LiteralPath $stdout | ForEach-Object { if ($_ -match '^AURA_INPUT_TICK=(\d+)$') { [uint32]$Matches[1] } })
@{ ownerLastUtc=$ownerLast.ToString('o'); ownTicks=$ticks; completedUtc=[DateTime]::UtcNow.ToString('o'); exitCode=$code; command="$exe --exact $Name --ignored --test-threads=1 --nocapture" } | ConvertTo-Json | Set-Content -LiteralPath $lastPath
Start-Sleep -Milliseconds 350
try { $after = Read-AuraOwnerWindow -Path $env:AURA_L18_OWNER_WINDOW -During }
catch { $after = [pscustomobject]@{condition=[pscustomobject]@{eligible=$false;revoked=$true;reason='Observer unavailable after test'}} }
if (-not $after.condition.eligible) { $interrupted=$true }
@{exitCode=$code; interrupted=$interrupted; after=$after.condition; command="$exe --exact $Name --ignored --test-threads=1 --nocapture";stdout=$stdout;stderr=$stderr;idle=$statePath;manifest=$ManifestPath} | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath ($stdout + '.result.json')
Write-Output "Exit code: $code; stdout: $stdout; stderr: $stderr; idle: $statePath"
Get-Content -LiteralPath $stdout -Tail 12
if ($interrupted) { Write-Output 'Stop this series: owner input or desktop/observer condition changed'; exit 3 }
exit $code
