/**
 * Where this installation keeps its data, Electron's own profile and Chrome's extension folder.
 * The launcher works them out and passes them; a desktop started without it (a development run,
 * an end-to-end test, `Aura.exe` started by hand) asks this version's Core, which works them out
 * the same way (`aura_data_location`). Never from the environment, never made up here.
 */
import { spawnSync } from 'node:child_process';

export interface DesktopLocations {
  readonly dataRoot: string;
  readonly uiProfile: string;
  readonly chromeRoot: string;
}

const LAUNCHER_ARGUMENTS: Readonly<Record<keyof DesktopLocations, string>> = {
  dataRoot: '--aura-data-root=',
  uiProfile: '--aura-ui-profile=',
  chromeRoot: '--aura-chrome-root=',
};

/** The launcher's three locations; `undefined` when it passed none, refused when only some. */
export function launcherLocations(argv: readonly string[]): DesktopLocations | undefined {
  const found = Object.entries(LAUNCHER_ARGUMENTS).map(
    ([key, prefix]) =>
      [key, argv.find((argument) => argument.startsWith(prefix))?.slice(prefix.length)] as const,
  );
  if (found.every(([, value]) => value === undefined)) return undefined;
  if (found.some(([, value]) => value === undefined || value.length === 0)) {
    throw new Error('the launcher passed an incomplete set of data locations');
  }
  return Object.fromEntries(found) as unknown as DesktopLocations;
}

/** How `aura-core --data-locations` ended and what it printed. */
export interface CoreAnswer {
  readonly status: number | null;
  readonly stdout: string;
  readonly error?: Error;
}

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === 'object' && value !== null && !Array.isArray(value);

const isPath = (value: unknown): value is string => typeof value === 'string' && value.length > 0;

/** The locations in Core's answer; its refusal, or an answer that cannot be used, is thrown. */
export function locationsFromCore(answer: CoreAnswer): DesktopLocations {
  if (answer.error !== undefined) {
    throw new Error(`Aura Core could not be asked where the data is: ${answer.error.message}`);
  }
  let parsed: unknown;
  try {
    parsed = JSON.parse(answer.stdout);
  } catch {
    throw new Error(
      `Aura Core's answer about the data locations is unreadable (exit ${answer.status})`,
    );
  }
  if (isRecord(parsed) && isRecord(parsed.error) && typeof parsed.error.message === 'string') {
    const { code, location } = parsed.error;
    throw Object.assign(new Error(parsed.error.message), { code, location });
  }
  const locations = isRecord(parsed) ? parsed.locations : undefined;
  if (answer.status !== 0 || !isRecord(locations)) {
    throw new Error(`Aura Core gave no data locations (exit ${answer.status})`);
  }
  const { data_root: dataRoot, ui_profile: uiProfile, chrome_root: chromeRoot } = locations;
  if (!isPath(dataRoot) || !isPath(uiProfile) || !isPath(chromeRoot)) {
    throw new Error('Aura Core gave an incomplete set of data locations');
  }
  return { dataRoot, uiProfile, chromeRoot };
}

/** The launcher's locations when it passed them; otherwise Core is asked. */
export function desktopLocations(
  argv: readonly string[],
  askCore: () => CoreAnswer,
): DesktopLocations {
  return launcherLocations(argv) ?? locationsFromCore(askCore());
}

/** Runs this version's Core for its answer; Main waits for it before the app is ready. */
export function askCore(coreExecutable: string, manifestPath: string): CoreAnswer {
  const result = spawnSync(
    coreExecutable,
    ['--data-locations', `--component-manifest=${manifestPath}`],
    { encoding: 'utf8', shell: false, timeout: 15_000, windowsHide: true },
  );
  return {
    status: result.status,
    stdout: result.stdout ?? '',
    ...(result.error === undefined ? {} : { error: result.error }),
  };
}
