import type { ElectronApplication } from '@playwright/test';

/**
 * Copies go to Main's clipboard. Tests replace its writeText in Main, so the owner's real
 * clipboard is never touched; if
 * the replacement does not hold, this throws before anything can be copied.
 */
export async function stubMainClipboard(app: ElectronApplication): Promise<void> {
  await app.evaluate(({ clipboard }) => {
    const copied: string[] = [];
    const stand = (text: string) => void copied.push(text);
    Reflect.set(globalThis, 'auraTestCopied', copied);
    Reflect.set(clipboard, 'writeText', stand);
    const current: unknown = Reflect.get(clipboard, 'writeText');
    if (current !== stand) throw new Error('Main clipboard could not be replaced');
  });
}

/** What was copied through Main since `stubMainClipboard`. */
export const copiedThroughMain = (app: ElectronApplication): Promise<string[]> =>
  app.evaluate(() => Reflect.get(globalThis, 'auraTestCopied') as string[]);
