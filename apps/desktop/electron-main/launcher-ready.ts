import { mkdir, rename, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';

export const LAUNCHER_READY_FILE = join('launcher', 'ready.v1.json');

export interface LauncherReadyReporter {
  coreState(state: string): Promise<void>;
  windowShown(): Promise<void>;
}

/**
 * Tells AuraLauncher that this release started: once Core is ready and the window has shown,
 * `launcher/ready.v1.json` in the data folder names the release. A version the launcher is trying
 * becomes the active one when this appears; one that never gets here is put back.
 */
export function createLauncherReadyReporter(options: {
  dataRoot: string;
  releaseBundleId: string;
  productVersion: string;
  write?: (path: string, text: string) => Promise<void>;
}): LauncherReadyReporter {
  let coreReady = false;
  let windowShown = false;
  let reported = false;
  const write = options.write ?? replaceFile;
  const report = async (): Promise<void> => {
    if (reported || !coreReady || !windowShown) return;
    reported = true;
    await write(
      join(options.dataRoot, LAUNCHER_READY_FILE),
      `${JSON.stringify({
        release_bundle_id: options.releaseBundleId,
        product_version: options.productVersion,
        ready_at: new Date().toISOString(),
      })}\n`,
    );
  };
  return {
    coreState(state) {
      if (state === 'ready') coreReady = true;
      return report();
    },
    windowShown() {
      windowShown = true;
      return report();
    },
  };
}

async function replaceFile(path: string, text: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  const temporary = `${path}.tmp`;
  await writeFile(temporary, text, 'utf8');
  await rename(temporary, path);
}
