/**
 * The lines of a Shell script or terminal input that name a place outside the folder they run
 * in — a drive, a share, a parent folder, a home or environment folder, the registry — shown
 * under the warning that the command writes outside the workspace. Core's check
 * (crates/aura-agent-execution/src/agent_execution/shell_risks/outside.rs) is what asks; this only points
 * at the lines.
 */
const PLACE = /(?:[a-z]:[\\/]|\\\\|\.\.[\\/]|~[\\/]|\$env:|\$home\b|%\w+%|\bhk(?:cu|lm|cr|u|cc):)/i;

/** At most this many lines are listed. */
const SHOWN = 6;

export interface OutsideLine {
  line: number;
  text: string;
}

export function outsideLines(script: string): OutsideLine[] {
  return script
    .split(/\r?\n/u)
    .flatMap((text, index) => (PLACE.test(text) ? [{ line: index + 1, text: text.trim() }] : []))
    .slice(0, SHOWN);
}
