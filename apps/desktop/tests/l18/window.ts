import { expect } from '@playwright/test';
import { spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';

/** Aura's own test window, started by the test as the user would start a program. */
export interface TestWindow {
  /** What happened in the window since it showed, one line a thing (see the fixture's notes). */
  events(): string[];
  close(): void;
}

function lines(log: string): string[] {
  try {
    return readFileSync(log, 'utf8').split(/\r?\n/u).filter(Boolean);
  } catch {
    return [];
  }
}

/** Starts the program with `--log` and waits until its window shows. */
export async function openTestWindow(
  program: string,
  log: string,
  args: string[],
): Promise<TestWindow> {
  const child = spawn(program, ['--log', log, ...args], { stdio: 'ignore' });
  const window: TestWindow = {
    events: () => {
      const all = lines(log);
      return all.slice(all.indexOf('ready') + 1);
    },
    close: () => {
      if (child.exitCode === null) child.kill();
    },
  };
  try {
    await expect.poll(() => lines(log).includes('ready'), { timeout: 20_000 }).toBe(true);
  } catch (error) {
    window.close();
    throw error;
  }
  return window;
}
