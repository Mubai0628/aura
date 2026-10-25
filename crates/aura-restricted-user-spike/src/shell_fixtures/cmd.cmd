@echo off
setlocal EnableExtensions DisableDelayedExpansion
if defined AURA_PARENT_ENV_CANARY exit /b 81
if not "%AURA_EXECUTION_ENVIRONMENT%"=="aura-shell-environment-v1" exit /b 82
set "R06_LITERAL=hello & world!"
mkdir "cmd space"
if errorlevel 1 exit /b 83
if not "%R06_LITERAL%"=="hello & world!" exit /b 88
>"cmd space\text.txt" echo hello ^& world!
if errorlevel 1 exit /b 84
call "r06-child.cmd"
if not errorlevel 9 exit /b 85
if errorlevel 10 exit /b 86
pwsh -MTA -NoLogo -NoProfile -NonInteractive -File r06-cmd-child.ps1
if errorlevel 1 exit /b 87
echo {"shell":"cmd","child_exit":9,"environment_revision":"%AURA_EXECUTION_ENVIRONMENT%"}
exit /b 0
