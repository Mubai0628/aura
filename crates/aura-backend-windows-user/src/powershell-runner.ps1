# The model's script is in script.ps1 beside this file. It is parsed here, after the
# bootstrap made the console UTF-8, so a parse error names the script's own line; it then runs as a
# script block, so its errors name its own lines too, and it exits as it would have inline.
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
$AuraScript = [scriptblock]::Create($AuraScriptText)
Remove-Variable -Name AuraScriptText, AuraParseTokens, AuraParseErrors
. $AuraScript
