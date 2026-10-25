/**
 * The kinds of
 * file Windows runs, or opens in something that runs what they hold, when they are "opened": Aura
 * never opens them with their default application. Main enforces this; the Renderer uses the same
 * list to say so before it asks.
 */
const RUNS = new Set(
  (
    'exe com bat cmd btm ps1 psm1 psd1 ps1xml psc1 pssc vbs vbe js jse wsf wsh ws wsc msc msi ' +
    'msp mst scr pif hta cpl jar lnk url website reg inf scf sct shb shs application appref-ms ' +
    'appx appxbundle msix msixbundle appinstaller gadget chm hlp py pyw pyz pyzw rb pl sh bash ' +
    'zsh ksh csh vb settingcontent-ms library-ms search-ms searchconnector-ms diagcab xbap xll ' +
    'wsb iso img vhd vhdx'
  ).split(' '),
);

/** The file's kind is one Aura never opens with its default application. */
export function runsCode(path: string): boolean {
  const name = path.split(/[\\/]/u).at(-1) ?? '';
  const dot = name.lastIndexOf('.');
  return dot > 0 && RUNS.has(name.slice(dot + 1).toLowerCase());
}

/**
 * A workspace-relative path as the conversation spells it: parts joined by `/`, none of them
 * empty, `.` or `..`, a Windows device name, or ending in a dot or a space (Windows would quietly
 * drop those and name another file). Its parts, or nothing.
 */
export function workspaceRelativeParts(path: string): string[] | undefined {
  if (!path || path.length > 1024) return undefined;
  const parts = path.split('/');
  for (const part of parts) {
    if (!part || part === '.' || part === '..') return undefined;
    // eslint-disable-next-line no-control-regex -- control characters are what is refused.
    if (/[<>:"\\|?*\u0000-\u001f]/u.test(part)) return undefined;
    if (/[. ]$/u.test(part)) return undefined;
    if (/^(?:con|prn|aux|nul|com[0-9¹²³]|lpt[0-9¹²³])(?:\..*)?$/iu.test(part)) return undefined;
  }
  return parts;
}
