param([string]$Archive)
if($PSVersionTable.PSVersion.Major -lt 7){throw 'Preparation requires PowerShell 7'}
. (Join-Path $PSScriptRoot 'common.ps1')
. (Join-Path $PSScriptRoot 'runtime-package.ps1')
$pin=Get-R06RuntimePin
$stage=Get-R06RuntimeStage
Assert-R06RuntimeAncestors $stage
if(Test-Path -LiteralPath $stage){Get-R06PreparedRuntime | ConvertTo-Json -Depth 5;exit 0}
if(-not $Archive){throw 'Supply the pinned official ZIP using -Archive; no implicit network download'}
$Archive=[IO.Path]::GetFullPath($Archive)
if((Get-Item -LiteralPath $Archive).Length -ne $pin.archive_bytes -or (Get-FileHash -LiteralPath $Archive -Algorithm SHA256).Hash -ne $pin.archive_sha256){throw 'Official runtime archive hash/size mismatch'}
$zip=[IO.Compression.ZipFile]::OpenRead($Archive)
try {
    $files=@();$seen=[Collections.Generic.HashSet[string]]::new([StringComparer]::OrdinalIgnoreCase)
    [long]$expanded=0
    if($zip.Entries.Count -gt $pin.max_entries){throw 'Archive entry limit exceeded'}
    foreach($entry in $zip.Entries){
        $name=$entry.FullName.TrimEnd('/')
        Assert-R06RuntimePath $name
        if(-not $seen.Add($name) -or (($entry.ExternalAttributes -shr 16) -band 0xf000) -eq 0xa000){throw 'Duplicate path or symbolic link in archive'}
        $expanded+=$entry.Length
        if($expanded -gt $pin.max_expanded_bytes){throw 'Expanded archive limit exceeded'}
        if($entry.FullName.EndsWith('/')){continue}
        $stream=$entry.Open()
        try{$hash=[Convert]::ToHexString([Security.Cryptography.SHA256]::HashData($stream)).ToLowerInvariant()}finally{$stream.Dispose()}
        $files+=,[pscustomobject]@{path=$entry.FullName;bytes=$entry.Length;sha256=$hash}
    }
    $manifest=[pscustomobject]@{schema='r06-runtime-files-v1';id=$pin.id;version=$pin.version;archive_sha256=$pin.archive_sha256;files=$files}
    Assert-R06RuntimeManifest $manifest
    $parent=Split-Path -Parent $stage
    New-Item -ItemType Directory -Path $parent -Force | Out-Null
    $temporary=Join-Path $parent ('.prepare-'+[guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path $temporary | Out-Null
    # Failure preserves the uniquely named staging tree for inspection.
    $payload=Join-Path $temporary 'payload';New-Item -ItemType Directory -Path $payload | Out-Null
    foreach($file in $files){
        $destination=Join-Path $payload $file.path
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        [IO.Compression.ZipFileExtensions]::ExtractToFile($zip.GetEntry($file.path),$destination,$false)
    }
    Assert-R06RuntimeFiles $payload $manifest
    $signature=Get-AuthenticodeSignature -LiteralPath (Join-Path $payload $pin.executable)
    if($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch '(?:^|, )O=Microsoft Corporation(?:,|$)'){throw 'PowerShell publisher signature verification failed'}
    $manifest | Add-Member -NotePropertyName publisher -NotePropertyValue ([pscustomobject]@{subject=$signature.SignerCertificate.Subject;thumbprint=$signature.SignerCertificate.Thumbprint;status='Valid'})
    Write-R06Json (Join-Path $temporary 'manifest.json') $manifest
    [IO.Directory]::Move($temporary,$stage)
    [pscustomobject]@{id=$pin.id;files=$files.Count;bytes=$expanded;path=$stage;publisher=$manifest.publisher;installed=$false} | ConvertTo-Json
}finally{$zip.Dispose()}
