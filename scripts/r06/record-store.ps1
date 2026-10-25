# The pending slot is uncommitted, installation-owned data. Never execute it.
# Atomic rename leaves the last committed record intact on process interruption.
function Write-R06CommittedRecord($Plan,$Record) {
    Assert-R06Root $Plan
    if($Record.install_id -cne $Plan.install_id -or $Record.owner_sid -cne $Plan.owner_sid){throw 'Record identity differs'}
    $path=Join-Path $Plan.root 'record.json'
    $pending=Join-Path $Plan.root 'record.pending.json'
    if(Test-Path -LiteralPath $pending){
        if((Get-Item -LiteralPath $pending -Force).Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Pending record is a reparse point'}
        # The fixed staging name is reserved by the bootstrap ledger. Unlink,
        # never truncate: a hardlink must not modify any other linked file.
        if(-not (Test-Path -LiteralPath $path) -and -not (Test-R06BootstrapSeed $Plan)){throw 'No committed record or bootstrap seed'}
        Remove-Item -LiteralPath $pending
    }
    $bytes=[Text.Encoding]::UTF8.GetBytes(($Record|ConvertTo-Json -Depth 24))
    $stream=[IO.File]::Open($pending,[IO.FileMode]::CreateNew,[IO.FileAccess]::Write,[IO.FileShare]::None)
    try{$stream.Write($bytes,0,$bytes.Length);$stream.Flush($true)}finally{$stream.Dispose()}
    Publish-R06CommittedRecord $pending $path
}
function Publish-R06CommittedRecord([string]$Pending,[string]$Destination) {
    [IO.File]::Move($Pending,$Destination,$true)
}
