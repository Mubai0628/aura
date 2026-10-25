# PowerShell reconstructs PSModulePath at startup even with an explicit env.
# Run before any module-backed command; the package is verified by the Runner.
$env:PSModulePath = [IO.Path]::Combine($PSHOME, 'Modules')
# PS7 appends .CPL during startup; retain Aura's native/batch discovery policy.
$env:PATHEXT = '.COM;.EXE;.BAT;.CMD'
$ErrorActionPreference = 'Stop'
$OutputEncoding = [Text.UTF8Encoding]::new($false)
[Console]::OutputEncoding = $OutputEncoding
[Console]::InputEncoding = $OutputEncoding
