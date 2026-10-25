Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Get-R06Layout {
    $repo = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '../..'))
    [pscustomobject]@{
        Repo = $repo
        Review = Join-Path $repo 'target/r06-review/plan.json'
        Source = Join-Path $repo 'target/debug/aura-restricted-user-spike.exe'
        Base = Join-Path ([Environment]::GetFolderPath('CommonApplicationData')) 'Aura/R06-Spike'
    }
}
function Get-R06Owner { [Security.Principal.WindowsIdentity]::GetCurrent().User.Value }
function Set-R06RunnerReportEncoding([Diagnostics.ProcessStartInfo]$Start) {
    # Rust emits UTF-8 JSON, regardless of the Windows ANSI/OEM code page.
    # A code-page decoder may consume a JSON quote after a multibyte character.
    $Start.StandardOutputEncoding=[Text.UTF8Encoding]::new($false,$true)
    $Start.StandardErrorEncoding=[Text.UTF8Encoding]::new($false,$true)
}
function Get-R06PlainLogText([string]$Text) {
    if($null -eq $Text){return ''}
    [regex]::Replace($Text,'\x1B(?:[@-Z\\-_]|\[(?:[0-?]*[ -/]*)[@-~])','')
}
function Write-R06Utf8Log([string]$Path,[string]$Text) {
    [IO.File]::WriteAllText($Path,(Get-R06PlainLogText $Text),[Text.UTF8Encoding]::new($false))
}
function Test-R06Administrator {
    [Security.Principal.WindowsPrincipal]::new([Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}
function Write-R06Json($Path, $Value) {
    $parent = Split-Path -Parent $Path
    $temporary = Join-Path $parent ('.write-' + [guid]::NewGuid().ToString('N'))
    $bytes = [Text.UTF8Encoding]::new($false).GetBytes(($Value | ConvertTo-Json -Depth 20))
    $stream = [IO.File]::Open($temporary, 'CreateNew', 'Write', 'None')
    try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) } finally { $stream.Dispose() }
    [IO.File]::Move($temporary, $Path, $true)
}
function Get-R06Plan {
    $layout = Get-R06Layout
    $plan = Get-Content -LiteralPath $layout.Review -Raw | ConvertFrom-Json
    if ($plan.schema -ne 'r06-review-v0' -or $plan.owner_sid -ne (Get-R06Owner)) { throw 'Review owner/schema mismatch; no host action allowed' }
    $id = [guid]::ParseExact($plan.install_id, 'D').ToString('D')
    $root = [IO.Path]::GetFullPath((Join-Path $layout.Base $id))
    if ($plan.root -ne $root -or $plan.account -ne 'AuraR06Offline' -or $plan.firewall -ne "Aura.R06.$id.Offline.Out") { throw 'Non-fixed resource request rejected' }
    if ($plan.runner_sha256 -notmatch '^[A-Fa-f0-9]{64}$') { throw 'Invalid reviewed Runner hash' }
    $plan
}
function Assert-R06Root($Plan) {
    $expected = [IO.Path]::GetFullPath((Join-Path (Get-R06Layout).Base $Plan.install_id))
    if ([IO.Path]::GetFullPath($Plan.root) -ne $expected) { throw 'Root outside fixed R06 installation directory' }
    $cursor = $expected
    while ($cursor) {
        if (Test-Path -LiteralPath $cursor) {
            if (((Get-Item -LiteralPath $cursor -Force).Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw "Reparse point in installation path: $cursor" }
        }
        $next = Split-Path -Parent $cursor
        if ($next -eq $cursor) { break }
        $cursor = $next
    }
}
function Get-R06PrivateDirectorySecurity($OwnerSid, $OfflineSid, $Writable) {
    $acl = [Security.AccessControl.DirectorySecurity]::new()
    $acl.SetAccessRuleProtection($true, $false)
    $acl.SetOwner([Security.Principal.SecurityIdentifier]::new($OwnerSid))
    foreach ($sid in @($OwnerSid, 'S-1-5-18', 'S-1-5-32-544')) {
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($sid), 'FullControl', 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    }
    if ($OfflineSid) {
        $rights = if ($Writable) { 'Modify' } else { 'ReadAndExecute' }
        $acl.AddAccessRule([Security.AccessControl.FileSystemAccessRule]::new([Security.Principal.SecurityIdentifier]::new($OfflineSid), $rights, 'ContainerInherit,ObjectInherit', 'None', 'Allow'))
    }
    $acl
}
function Set-R06PrivateDirectory($Path, $OwnerSid, $OfflineSid, $Writable) {
    # Only newly-created Aura-owned directories use a protected replacement DACL.
    # Submit only the owner/DACL sections changed by the factory. The PowerShell
    # provider may request SeSecurityPrivilege for unrelated SACL handling.
    [IO.FileSystemAclExtensions]::SetAccessControl([IO.DirectoryInfo]::new($Path),(Get-R06PrivateDirectorySecurity $OwnerSid $OfflineSid $Writable))
}
function Get-R06AccountDescription($Plan) {
    # LocalAccounts limits Description to 48 characters; retain the full UUID.
    $id=[guid]::ParseExact($Plan.install_id, 'D').ToString('D')
    "Aura R06 $id"
}
function Assert-R06Account($Plan, $Record) {
    $account = Get-LocalUser -Name $Plan.account -ErrorAction SilentlyContinue
    if ($account) {
        if ($account.Description -ne (Get-R06AccountDescription $Plan)) { throw 'Account name belongs to another owner' }
        if ($Record.offline_sid -and $account.SID.Value -ne $Record.offline_sid) { throw 'Offline account SID changed' }
    }
    $account
}
function Assert-R06Firewall($Plan, $Record) {
    $rule = Get-NetFirewallRule -PolicyStore PersistentStore -Name $Plan.firewall -ErrorAction SilentlyContinue
    if ($rule) {
        if ($rule.Description -ne "Aura R06 spike $($Plan.install_id)" -or $rule.Direction -ne 'Outbound' -or $rule.Action -ne 'Block' -or $rule.Profile -ne 'Any') { throw 'Firewall rule ownership/semantics changed' }
        $security = $rule | Get-NetFirewallSecurityFilter
        if ($security.LocalUser -ne "D:(A;;CC;;;$($Record.offline_sid))") { throw 'Firewall principal changed' }
        $application = $rule | Get-NetFirewallApplicationFilter
        $ports = $rule | Get-NetFirewallPortFilter
        $addresses = $rule | Get-NetFirewallAddressFilter
        if ($application.Program -ne 'Any' -or $ports.Protocol -ne 'Any' -or $addresses.RemoteAddress -ne 'Any' -or $addresses.LocalAddress -ne 'Any' -or $ports.LocalPort -ne 'Any' -or $ports.RemotePort -ne 'Any') { throw 'Firewall scope narrowed' }
    }
    $rule
}
function Get-R06ComparableDirectorySddl([string]$Sddl) {
    # SetSecurityInfo marks a protected DACL auto-inherited (PAI) after children
    # exist. That OS metadata bit is not an owner or ACE change.
    $Sddl -creplace 'D:PAI(?=\()','D:P'
}
function Assert-R06RecordedDirectorySddl([string]$Path,[string]$Expected,[string]$Message) {
    if(-not $Expected){return}
    if(-not (Test-Path -LiteralPath $Path)){throw $Message}
    $actual=Get-R06ComparableDirectorySddl ((Get-Acl -LiteralPath $Path).Sddl)
    $expected=Get-R06ComparableDirectorySddl $Expected
    if($actual -cne $expected){throw $Message}
}
function Read-R06OwnedRecord($Plan) {
    Assert-R06Root $Plan
    Assert-R06OwnedTree $Plan
    $record = Get-Content -LiteralPath (Join-Path $Plan.root 'record.json') -Raw | ConvertFrom-Json
    if ($record.schema -ne 'r06-setup-v0' -or $record.install_id -ne $Plan.install_id -or $record.owner_sid -ne $Plan.owner_sid) { throw 'Unowned Setup record' }
    if ($record.PSObject.Properties['account'] -and $record.account -and $record.account -cne $Plan.account) { throw 'Recorded account name changed' }
    $record
}
function Get-R06Record($Plan) {
    $record=Read-R06OwnedRecord $Plan
    foreach ($entry in @(@('root_dacl',$Plan.root),@('scratch_dacl',(Join-Path $Plan.root 'scratch')),@('private_dacl',(Join-Path $Plan.root 'private')))) {
        Assert-R06RecordedDirectorySddl $entry[1] $record.($entry[0]) 'Recorded resource DACL changed; preserve for review'
    }
    $record
}
function Save-R06Record($Plan, $Record) {
    . (Join-Path $PSScriptRoot 'record-store.ps1')
    Write-R06CommittedRecord $Plan $Record
}
function Bind-R06RecordAccount($Plan,$Record) {
    # Helper manifest::build reads record.account. The name lives on the Review
    # plan; older records omitted it and publication then failed with
    # installer.missing_account after WFP/NUL/Ksec were already applied.
    if(-not $Plan.account){throw 'Review account missing'}
    if($Record.PSObject.Properties['account'] -and $Record.account){
        if($Record.account -cne $Plan.account){throw 'Recorded account name changed'}
        return $Record
    }
    $Record|Add-Member -NotePropertyName account -NotePropertyValue $Plan.account -Force
    Save-R06Record $Plan $Record
    $Record
}
function Assert-R06NoProcesses($Sid) {
    if (-not $Sid) { return }
    foreach ($process in Get-CimInstance Win32_Process) {
        $owner = Invoke-CimMethod -InputObject $process -MethodName GetOwnerSid -ErrorAction SilentlyContinue
        if ($owner -and $owner.ReturnValue -eq 0 -and $owner.Sid -eq $Sid) { throw 'Offline processes remain; stop the owned Runner Job before Repair/Remove' }
    }
}
function Assert-R06OwnedTree($Plan) {
    Assert-R06Root $Plan
    foreach ($entry in Get-ChildItem -LiteralPath $Plan.root -Force) {
        if ($entry.Name -notin @('record.json','record.pending.json','runner.exe','runner.next.exe','private','scratch','runtime')) { throw 'Unknown install entry; preserve for review' }
    }
    $pending=[Collections.Generic.Stack[string]]::new(); $pending.Push($Plan.root)
    while ($pending.Count) {
        foreach ($entry in Get-ChildItem -LiteralPath $pending.Pop() -Force) {
            if (($entry.Attributes -band [IO.FileAttributes]::ReparsePoint) -ne 0) { throw 'Reparse point in owned resources; preserve for reconciliation' }
            if ($entry.PSIsContainer) { $pending.Push($entry.FullName) }
        }
    }
}
function Assert-R06FirewallService {
    if ((Get-Service -Name MpsSvc).Status -ne 'Running') { throw 'Windows Firewall service is not running; no automatic global repair' }
    if (@(Get-NetFirewallProfile -PolicyStore ActiveStore | Where-Object {$_.Enabled -ne $true}).Count) { throw 'Firewall profiles disabled; no launch or automatic global policy change' }
}

function Get-R06SystemCmdImage {
    # SpecialFolder.Windows queries Windows; never use parent PATH/COMSPEC.
    $system=[Environment]::GetFolderPath([Environment+SpecialFolder]::Windows)
    if(-not $system -or -not [IO.Path]::IsPathFullyQualified($system)){throw 'Windows directory unavailable'}
    $image=Join-Path $system 'System32/cmd.exe'
    [pscustomobject]@{name='cmd';path=$image;sha256=(Get-FileHash -LiteralPath $image -Algorithm SHA256).Hash}
}
