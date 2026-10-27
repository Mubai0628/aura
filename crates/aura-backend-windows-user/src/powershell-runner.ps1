# The model's script is in script.ps1 beside this file. It is parsed here, after the
# bootstrap made the console UTF-8, so a parse error names the script's own line; it then runs as a
# script block, so its errors name its own lines too.
# The run exits as a terminal shows it: `exit N` exits N, an uncaught error exits 1, and otherwise
# the last statement decides - a failing program last exits with its own code. `pwsh -File` would
# exit 0 after any normal end, so the status of the last statement is kept by a line appended after
# the script, where it shifts none of the script's own line numbers.
$PSStyle.OutputRendering = 'PlainText'
$AuraScriptText = [IO.File]::ReadAllText([IO.Path]::Combine($PSScriptRoot, 'script.ps1'))
$AuraParseTokens = $null
$AuraParseErrors = $null
$null = [System.Management.Automation.Language.Parser]::ParseInput($AuraScriptText, [ref]$AuraParseTokens, [ref]$AuraParseErrors)
if ($AuraParseErrors.Count -gt 0) {
    foreach ($AuraParseError in ($AuraParseErrors | Select-Object -First 5)) {
        $AuraAt = $AuraParseError.Extent.StartScriptPosition
        [Console]::Error.WriteLine("ParserError: line $($AuraAt.LineNumber), column $($AuraAt.ColumnNumber): $($AuraParseError.Message)")
        [Console]::Error.WriteLine("  $($AuraAt.Line.TrimEnd())")
    }
    exit 1
}
$AuraScript = [scriptblock]::Create($AuraScriptText + "`n" + '$global:AuraLastStatementSucceeded = $?')
Remove-Variable -Name AuraScriptText, AuraParseTokens, AuraParseErrors
$global:AuraLastStatementSucceeded = $true
$global:LASTEXITCODE = 0
. $AuraScript
if (-not $global:AuraLastStatementSucceeded) {
    if ($global:LASTEXITCODE) { exit $global:LASTEXITCODE }
    exit 1
}
