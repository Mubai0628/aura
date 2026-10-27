# PowerShell reconstructs PSModulePath at startup even with an explicit env.
# Run before any module-backed command; the package is verified by the Runner.
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
# PS7 appends .CPL during startup; retain Aura's native/batch discovery policy.
$env:PATHEXT = '.COM;.EXE;.BAT;.CMD'
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $OutputEncoding
[Console]::InputEncoding = $OutputEncoding
# Start-Process opens a console window of its own by default. A program it starts shares this
# run's hidden console instead, unless the call names a window (-WindowStyle, -NoNewWindow) or a
# verb, or the target is opened through the shell (an address or a document).
$AuraStartProcess = [System.Management.Automation.ProxyCommand]::Create(
    [System.Management.Automation.CommandMetadata]::new(
        (Get-Command -Name Start-Process -CommandType Cmdlet)))
$AuraStartProcess = $AuraStartProcess.Replace('$scriptCmd = {', @'
if (-not ($PSBoundParameters.ContainsKey('NoNewWindow') -or $PSBoundParameters.ContainsKey('WindowStyle') -or $PSBoundParameters.ContainsKey('Verb'))) {
    $AuraProgram = Get-Command -Name ([string]$PSBoundParameters['FilePath']) -CommandType Application -ErrorAction Ignore | Select-Object -First 1
    if ($AuraProgram -and [IO.Path]::GetExtension($AuraProgram.Source) -in '.exe', '.com', '.bat', '.cmd') {
        $PSBoundParameters['NoNewWindow'] = $true
    }
}
$scriptCmd = {
'@)
Set-Item -Path function:global:Start-Process -Value ([scriptblock]::Create($AuraStartProcess))
Remove-Variable -Name AuraStartProcess
