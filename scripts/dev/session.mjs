// Those
// reached `concurrently`, which on Windows ends every process below it by force: Electron and Core
// died together, Core never recorded a normal exit, and every start checked the whole Store (39 s
// on the owner's 2.2 GB Store). Here the renderer and Electron run in a process group of their
// own, so a Ctrl+C or a closed terminal reaches only this script, which ends Aura the way the
// window's × does: Main asks Core to record the normal exit, then stops it.
import { spawnSync } from 'node:child_process';

/** How long a closed window may take to end Electron and Core before they are ended by force. */
export const QUIT_TIMEOUT_MS = 15_000;

/**
 * Runs the renderer and Electron until either ends or the terminal asks to stop; then closes Aura's
 * window, stops the renderer, waits for Electron, and reports how the session ended.
 */
export function runDevSession({
  spawnRenderer,
  spawnElectron,
  closeAuraWindows,
  killTree,
  onExit,
  signals = process,
  quitTimeoutMs = QUIT_TIMEOUT_MS,
}) {
  const renderer = spawnRenderer();
  const electron = spawnElectron();
  const electronEnded = new Promise((resolve) => {
    if (electron.exitCode !== null) resolve(electron.exitCode);
    else electron.once('exit', (code) => resolve(code ?? 1));
  });
  let stopping = false;
  const stop = async (code) => {
    if (stopping) return;
    stopping = true;
    // The window's ×: Main records the normal exit with Core, then stops it. No window yet
    // (Electron is still building) means nothing to close and nothing to record.
    const asked = closeAuraWindows();
    // The renderer server is not needed to quit: the window has already loaded.
    killTree(renderer.pid);
    const ended = asked
      ? await Promise.race([
          electronEnded.then(() => true),
          new Promise((resolve) => setTimeout(() => resolve(false), quitTimeoutMs)),
        ])
      : false;
    if (!ended) killTree(electron.pid);
    onExit(code);
  };
  electronEnded.then((code) => stop(code));
  renderer.once('exit', (code) => stop(code || 1));
  for (const signal of ['SIGINT', 'SIGBREAK', 'SIGHUP']) signals.on(signal, () => stop(130));
}

/** Asks every window of this checkout's Electron to close, as its × does; returns how many. */
export function closeAuraWindows(repositoryRoot) {
  // Windows reports the process path with backslashes.
  const root = repositoryRoot.replaceAll('/', '\\').replaceAll("'", "''");
  const script = [
    `$root = '${root}'`,
    '$windows = @(Get-Process electron -ErrorAction SilentlyContinue | Where-Object {',
    '  $_.MainWindowHandle -ne 0 -and $_.Path -and',
    '  $_.Path.StartsWith($root, [StringComparison]::OrdinalIgnoreCase) })',
    'foreach ($window in $windows) { [void]$window.CloseMainWindow() }',
    '$windows.Count',
  ].join('\n');
  const result = spawnSync(
    'powershell.exe',
    ['-NoProfile', '-NonInteractive', '-Command', script],
    {
      encoding: 'utf8',
      timeout: 10_000,
      windowsHide: true,
    },
  );
  return Number.parseInt(result.stdout?.trim() ?? '', 10) || 0;
}

/** Ends a process and everything it started, by force. */
export function killTree(pid) {
  if (!pid) return;
  spawnSync('taskkill', ['/PID', String(pid), '/T', '/F'], { stdio: 'ignore', windowsHide: true });
}
