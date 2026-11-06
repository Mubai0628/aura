function Read-AuraJson {
    param([Parameter(Mandatory=$true)][string]$Path)
    $sharing = [IO.FileShare]::ReadWrite -bor [IO.FileShare]::Delete
    $stream = [IO.FileStream]::new($Path,[IO.FileMode]::Open,[IO.FileAccess]::Read,$sharing)
    try {
        $reader = [IO.StreamReader]::new($stream)
        try { return ($reader.ReadToEnd() | ConvertFrom-Json) }
        finally { $reader.Dispose() }
    } finally { $stream.Dispose() }
}

# How long no input of the owner's must have reached the desktop before a test may take it.
$AuraOwnerAwayMs = 60000

function Get-AuraOwnerCondition {
    param($Window, $Ledger, $Idle, [DateTimeOffset]$Now = [DateTimeOffset]::UtcNow, [switch]$During)
    try {
        $age = ($Now - ([DateTimeOffset]$Ledger.atUtc).ToUniversalTime()).TotalMilliseconds
        $ownerLast = ([DateTimeOffset]$Ledger.ownerLastUtc).ToUniversalTime()
        $ownerIdle = ($Now - $ownerLast).TotalMilliseconds
        $reason = if ($Window.revoked) { 'This departure window was already stopped' }
            elseif (-not $Ledger.healthy -or $age -lt 0 -or $age -gt 5000) { 'Input observer is unavailable or stale' }
            elseif (-not $Idle.unlocked) { 'Desktop locked' }
            elseif ($Ledger.otherEvents -ne $Window.initialOtherEvents) { 'Unattributed input after this departure window started' }
            else { '' }
        $attributed = $Idle.idleMs -ge $AuraOwnerAwayMs -or @($Ledger.ownTicks) -contains $Idle.lastInputTick
        $eligible = -not $reason -and $ownerIdle -ge $AuraOwnerAwayMs -and ($During -or $attributed)
        return [pscustomobject]@{ eligible=$eligible; revoked=[bool]$reason; reason=$reason; ownerIdleMs=$ownerIdle }
    } catch {
        return [pscustomobject]@{ eligible=$false; revoked=$true; reason='Departure metadata unavailable'; ownerIdleMs=0 }
    }
}

function Read-AuraOwnerWindow {
    param([Parameter(Mandatory=$true)][string]$Path, [switch]$During)
    $window = Read-AuraJson $Path
    $ledger = Read-AuraJson $window.ledger
    # A consumed fixture Escape never reaches a later hook; its driver records the sent tick.
    if ($window.ownInputRecord -and (Test-Path -LiteralPath $window.ownInputRecord)) {
        $record = Read-AuraJson $window.ownInputRecord
        if (([DateTimeOffset]$record.completedUtc) -ge ([DateTimeOffset]$window.armedUtc)) {
            $ledger.ownTicks = @($ledger.ownTicks) + @($record.ownTicks)
        }
    }
    $idle = Get-AuraDesktopIdle
    $condition = Get-AuraOwnerCondition -Window $window -Ledger $ledger -Idle $idle -During:$During
    if ($condition.revoked -and -not $window.revoked) {
        $window.revoked = $true
        $window | Add-Member -NotePropertyName reason -NotePropertyValue $condition.reason -Force
        $window | Add-Member -NotePropertyName revokedUtc -NotePropertyValue ([DateTimeOffset]::UtcNow.ToString('o')) -Force
        $window | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $Path
    }
    return [pscustomobject]@{ condition=$condition; idle=$idle; ledger=$ledger; window=$window }
}
