import { expect } from '@playwright/test';
import { spawn, type ChildProcess } from 'node:child_process';
import { join, resolve } from 'node:path';

const repository = resolve(import.meta.dirname, '../../../..');
export const SKILL = join(repository, 'resources/skills/write-aura-plugin');

export type Sent = Record<string, unknown>;

/** A fake chat platform on this computer, as a channel adapter and a tester see it. */
export interface Platform {
  /** Where the adapter connects. */
  address: string;
  /** A user's message on the platform. */
  push(message: Record<string, unknown>): Promise<void>;
  /** Every message the bot posted, oldest first. */
  sent(): Promise<Sent[]>;
  /** The first message the bot posted that `matches`, once it did. */
  posted(what: string, matches: (sent: Sent) => boolean): Promise<Sent>;
  /** Every command menu the bot set, oldest first; none where the platform has no menu. */
  menus(): Promise<Sent[]>;
  close(): void;
}

/** The process's first output line that `port` finds a port in. */
async function portOf(child: ChildProcess, port: (line: string) => number | undefined) {
  return new Promise<number>((done, failed) => {
    let output = '';
    child.stdout?.setEncoding('utf8');
    child.stdout?.on('data', (part: string) => {
      output += part;
      const found = port(output);
      if (found !== undefined) done(found);
    });
    child.once('exit', (code) => failed(new Error(`exited ${code}: ${output}`)));
    child.once('error', failed);
  });
}

function platform(child: ChildProcess, base: string, address: string): Platform {
  const sent = async () => {
    const answer = await fetch(`${base}/sent`);
    return ((await answer.json()) as { sent: Sent[] }).sent;
  };
  return {
    address,
    async push(message) {
      const answer = await fetch(`${base}/push`, {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify(message),
      });
      expect(answer.ok).toBe(true);
    },
    sent,
    async posted(what, matches) {
      let found: Sent | undefined;
      await expect
        .poll(async () => (found = (await sent()).find(matches)) !== undefined, {
          message: what,
          timeout: 90_000,
        })
        .toBe(true);
      return found as Sent;
    },
    async menus() {
      const answer = await fetch(`${base}/menus`);
      return answer.ok ? ((await answer.json()) as { menus: Sent[] }).menus : [];
    },
    close: () => void child.kill(),
  };
}

/** The Skill's long-polling platform (`platform/fake_platform.py`). */
export async function pollingPlatform(): Promise<Platform> {
  const script = join(SKILL, 'examples/fake-chat/platform/fake_platform.py');
  const child = spawn('python', [script, '--port', '0'], {
    env: { ...process.env, PYTHONDONTWRITEBYTECODE: '1', PYTHONIOENCODING: 'utf-8' },
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  const port = await portOf(child, (output) => {
    const found = /127\.0\.0\.1:(\d+)/u.exec(output);
    return found ? Number(found[1]) : undefined;
  });
  const base = `http://127.0.0.1:${port}`;
  return platform(child, base, base);
}

/** The WebSocket platform of these tests (`fake-ws-platform.mjs`). */
export async function socketPlatform(): Promise<Platform> {
  const script = join(import.meta.dirname, 'fake-ws-platform.mjs');
  const child = spawn(process.execPath, [script, '--port', '0'], {
    stdio: ['ignore', 'pipe', 'ignore'],
  });
  const port = await portOf(child, (output) => {
    const found = /listening (\d+)/u.exec(output);
    return found ? Number(found[1]) : undefined;
  });
  return platform(child, `http://127.0.0.1:${port}`, `ws://127.0.0.1:${port}/gateway`);
}
