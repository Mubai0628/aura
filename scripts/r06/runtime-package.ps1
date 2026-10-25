# Pure package validation shared by preparation, Review and host lifecycle.
function Get-R06RuntimePin {
    Get-Content -LiteralPath (Join-Path (Get-R06Layout).Repo 'runtimes/powershell.json') -Raw | ConvertFrom-Json
}
function Get-R06RuntimeStage {
    Join-Path (Get-R06Layout).Repo ('target/managed-runtimes/'+(Get-R06RuntimePin).id)
}
function Assert-R06RuntimeAncestors([string]$Path) {
    $cursor=[IO.Path]::GetFullPath($Path)
    while($cursor){
        if((Test-Path -LiteralPath $cursor) -and ((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Reparse point in runtime path'}
        $next=Split-Path -Parent $cursor
        if($next -eq $cursor){break};$cursor=$next
    }
}
function Assert-R06RuntimePath([string]$Path) {
    if(-not $Path -or $Path.Contains('\') -or $Path.StartsWith('/') -or $Path.Contains(':')){throw 'Unsafe runtime relative path'}
    foreach($part in $Path.Split('/')){
        if($part -in @('','.','..') -or $part -match '[<>"|?*\x00-\x1f]' -or $part.EndsWith('.') -or $part.EndsWith(' ') -or $part -match '^(?i:con|prn|aux|nul|com[0-9]|lpt[0-9])(?:\.|$)'){throw 'Unsafe runtime path component'}
    }
}
function Assert-R06RuntimeManifest($Manifest) {
    $pin=Get-R06RuntimePin
    if($Manifest.schema -ne 'r06-runtime-files-v1' -or $Manifest.id -ne $pin.id -or $Manifest.archive_sha256 -ne $pin.archive_sha256 -or $Manifest.version -ne $pin.version){throw 'Runtime manifest pin mismatch'}
    $paths=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$total=0
    foreach($file in $Manifest.files){
        Assert-R06RuntimePath $file.path
        if(-not $paths.Add($file.path) -or $file.sha256 -notmatch '^[a-fA-F0-9]{64}$' -or $file.bytes -lt 0){throw 'Invalid runtime file manifest'}
        $total+=$file.bytes
    }
    if($paths.Count -gt $pin.max_entries -or $total -gt $pin.max_expanded_bytes){throw 'Runtime package limit exceeded'}
    foreach($required in @($pin.executable,$pin.license,$pin.notices)){if(-not $paths.Contains($required)){throw 'Runtime executable/license/notices missing'}}
    # A file may not simultaneously be a parent directory, including case aliases.
    foreach($file in $Manifest.files){
        $parent=$file.path
        while($parent.Contains('/')){$parent=$parent.Substring(0,$parent.LastIndexOf('/'));if($paths.Contains($parent)){throw 'Runtime file/directory collision'}}
    }
    $lines=[string[]]@($Manifest.files | ForEach-Object {$_.path+'|'+$_.bytes.ToString([Globalization.CultureInfo]::InvariantCulture)+'|'+$_.sha256.ToLowerInvariant()})
    [Array]::Sort($lines,[StringComparer]::Ordinal)
    $digest=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData([Text.Encoding]::UTF8.GetBytes(($lines -join "`n")))).ToLowerInvariant()
    if($digest -ne $pin.files_sha256){throw 'Runtime file manifest differs from pinned archive contents'}
}
function Get-R06RuntimeInventory([string]$Root) {
    Assert-R06RuntimeAncestors $Root
    $rootItem=Get-Item -LiteralPath $Root -Force
    if(-not $rootItem.PSIsContainer -or ($rootItem.Attributes -band [IO.FileAttributes]::ReparsePoint)){throw 'Runtime root is not a plain directory'}
    $pending=[Collections.Generic.Stack[string]]::new();$pending.Push($rootItem.FullName)
    while($pending.Count){
        foreach($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force){
            if($entry.Attributes -band [IO.FileAttributes]::ReparsePoint){throw 'Runtime reparse point rejected'}
            if($entry.PSIsContainer){$pending.Push($entry.FullName)}
            $relative=[IO.Path]::GetRelativePath($rootItem.FullName,$entry.FullName).Replace('\','/')
            Assert-R06RuntimePath $relative
            [pscustomobject]@{path=$relative;directory=$entry.PSIsContainer;full=$entry.FullName}
        }
    }
}
function Assert-R06RuntimeFiles([string]$Root,$Manifest,[switch]$Partial,[string]$JournaledStage) {
    Assert-R06RuntimeManifest $Manifest
    $files=@{};$directories=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    foreach($file in $Manifest.files){
        $files[$file.path]=$file;$parent=$file.path
        while($parent.Contains('/')){$parent=$parent.Substring(0,$parent.LastIndexOf('/'));$null=$directories.Add($parent)}
    }
    $count=0
    foreach($entry in Get-R06RuntimeInventory $Root){
        if($entry.directory){if(-not $directories.Contains($entry.path)){throw 'Unknown runtime directory; preserve it'};continue}
        if($JournaledStage -and $entry.path -ceq $JournaledStage){continue} # Caller validates the pinned prefix and journal first.
        if(-not $files.ContainsKey($entry.path)){throw 'Unknown runtime file; preserve it'}
        $expected=$files[$entry.path]
        if((Get-Item -LiteralPath $entry.full).Length -ne $expected.bytes -or (Get-FileHash -LiteralPath $entry.full -Algorithm SHA256).Hash -ne $expected.sha256){throw 'Runtime file changed; preserve it'}
        $count++
    }
    if(-not $Partial -and $count -ne $files.Count){throw 'Runtime files missing'}
}
function Get-R06PreparedRuntime {
    $stage=Get-R06RuntimeStage
    $manifest=Get-Content -LiteralPath (Join-Path $stage 'manifest.json') -Raw | ConvertFrom-Json
    Assert-R06RuntimeFiles (Join-Path $stage 'payload') $manifest
    $manifest
}
