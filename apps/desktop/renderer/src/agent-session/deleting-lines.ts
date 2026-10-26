/**
 * The lines of a Shell script that delete, shown under the warning. The patterns
 * are Core's (crates/aura-core/src/agent_execution/shell_risks/deletion.rs), read line by line; Core's
 * check is what asks, this only points at the lines.
 */
const COMMAND =
  /(?:^|[;|&({])\s*(?:&\s*)?(?:remove-item|ri|rm|rmdir|rd|del|erase|clear-recyclebin|remove-itemproperty\s+-path\s+\S*::filesystem)(?:\.exe)?(?:\s|$|[;|&)}])/i;
const DOTNET =
  /\[(?:system\.)?io\.(?:file|directory)\]::delete\s*\(|\]::delete(?:file|directory)\s*\(|\.delete\s*\(\s*(?:\$true|\$false)?\s*\)/i;
const TOOLS =
  /\bgit(?:\.exe)?\s+clean\b[^;|&]*\s-(?:-force|[a-z]*f)|\brobocopy(?:\.exe)?\b[^;|&]*\s\/(?:mir|purge)\b|\bcmd(?:\.exe)?\s+\/[ck]\s+.*\b(?:del|erase|rd|rmdir)\b/i;

/** At most this many lines are listed. */
const SHOWN = 6;

export interface DeletingLine {
  line: number;
  text: string;
}

export function deletingLines(script: string): DeletingLine[] {
  return script
    .split(/\r?\n/u)
    .flatMap((text, index) =>
      [COMMAND, DOTNET, TOOLS].some((pattern) => pattern.test(text))
        ? [{ line: index + 1, text: text.trim() }]
        : [],
    )
    .slice(0, SHOWN);
}
