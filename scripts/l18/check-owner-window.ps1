param([switch]$During)
$ErrorActionPreference='Stop'
. (Join-Path $PSScriptRoot 'desktop-idle.ps1')
. (Join-Path $PSScriptRoot 'owner-window.ps1')
if (-not $env:AURA_L18_OWNER_WINDOW) { throw 'Recorded departure window is required' }
$state=Read-AuraOwnerWindow -Path $env:AURA_L18_OWNER_WINDOW -During:$During
@{atUtc=[DateTimeOffset]::UtcNow.ToString('o');condition=$state.condition;systemIdleMs=$state.idle.idleMs;unlocked=$state.idle.unlocked} | ConvertTo-Json -Compress
if(-not $state.condition.eligible){exit 3}
exit 0
