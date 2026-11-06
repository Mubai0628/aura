$ErrorActionPreference = 'Stop'
. (Join-Path $PSScriptRoot 'owner-window.ps1')
$now = [DateTimeOffset]::Parse('2026-11-03T03:30:00Z')
function Check($Name, $Mutate, [bool]$Expected, [bool]$Revoked) {
    $window = @{ initialOtherEvents=0; revoked=$false }
    $ledger = @{ atUtc=$now.ToString('o'); ownerLastUtc=$now.AddMinutes(-11).ToString('o'); otherEvents=0; ownTicks=@(10); healthy=$true }
    $idle = @{ unlocked=$true; idleMs=660000; lastInputTick=9 }
    & $Mutate $window $ledger $idle
    $actual = Get-AuraOwnerCondition -Window $window -Ledger $ledger -Idle $idle -Now $now
    if ($actual.eligible -ne $Expected -or $actual.revoked -ne $Revoked) {
        Write-Output "FAIL $Name expected eligible=$Expected revoked=$Revoked got eligible=$($actual.eligible) revoked=$($actual.revoked)"
        $script:failed++
    } else { Write-Output "PASS $Name" }
}
$script:failed=0
Check 'idle and unlocked starts the window' {} $true $false
Check 'Aura input cannot reset owner absence' {param($w,$l,$i) $i.idleMs=1; $i.lastInputTick=10} $true $false
Check 'short owner absence cannot use Aura input to bypass threshold' {param($w,$l,$i) $i.idleMs=1; $i.lastInputTick=10; $l.ownerLastUtc=$now.AddSeconds(-50).ToString('o')} $false $false
Check 'one minute away is enough' {param($w,$l,$i) $i.idleMs=61000; $l.ownerLastUtc=$now.AddSeconds(-61).ToString('o')} $true $false
Check 'unknown input remains detected after newer Aura input' {param($w,$l,$i) $l.otherEvents=1; $i.lastInputTick=10; $i.idleMs=1} $false $true
Check 'stale observer revokes the window' {param($w,$l,$i) $l.atUtc=$now.AddSeconds(-6).ToString('o')} $false $true
Check 'lock revokes the window' {param($w,$l,$i) $i.unlocked=$false} $false $true
Check 'a revoked window never silently resumes' {param($w,$l,$i) $w.revoked=$true} $false $true
Check 'explicit UTC offset preserves the one minute rule' {param($w,$l,$i) $l.ownerLastUtc='2026-11-03T11:28:50+08:00'; $i.lastInputTick=10; $i.idleMs=1} $true $false
Check 'explicit UTC offset still counts a short absence' {param($w,$l,$i) $l.ownerLastUtc='2026-11-03T11:29:30+08:00'; $i.lastInputTick=10; $i.idleMs=1} $false $false
if($script:failed){exit 1}
exit 0
