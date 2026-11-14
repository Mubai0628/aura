// @vitest-environment node
import { expect, test, vi } from 'vitest';
import { spawn } from 'node:child_process';
import { appendFile } from 'node:fs/promises';
import { createFixture } from './core-supervisor-tests/fixture.js';

vi.mock('node:child_process', () => ({ spawn: vi.fn() }));
vi.mock('node:fs/promises', () => ({ mkdir: vi.fn(), appendFile: vi.fn() }));
vi.mock('./core-transport.js', () => ({ CoreTransport: { connect: vi.fn() } }));

// Core is told the data folder Main was given, so it can check it against its own answer, and
// Main's log lines go into that folder.
test('every Core is started with the data folder Main was given and logs into it', async () => {
  const dataRoot = 'D:\\Moved Aura\\data';
  const f = await createFixture({ dataRoot });
  await f.supervisor.start();
  const [, args] = vi.mocked(spawn).mock.calls[0]!;
  expect(args as string[]).toContain(`--data-root=${dataRoot}`);
  const logged = vi.mocked(appendFile).mock.calls.map(([path]) => String(path));
  expect(logged.length).toBeGreaterThan(0);
  for (const path of logged) expect(path).toBe(`${dataRoot}\\logs\\main.jsonl`);
});
