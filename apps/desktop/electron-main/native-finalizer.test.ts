// @vitest-environment node
import { spawn, type ChildProcess } from 'node:child_process';
import { randomBytes } from 'node:crypto';
import { mkdtemp, rm, writeFile } from 'node:fs/promises';
import { createServer, type Socket } from 'node:net';
import { tmpdir } from 'node:os';
import { resolve, join } from 'node:path';
import { afterEach, expect, it } from 'vitest';
import { connectMainPipe } from './main-pipe-bridge.js';

// The worker process runs inside its folder; Windows can hold the folder a moment after it ends.
const roots: string[] = [];
afterEach(async () => {
  await Promise.all(
    roots.splice(0).map((root) => rm(root, { recursive: true, force: true, maxRetries: 5 })),
  );
});

it('L03 B0 normal close settles pending native read and write', async () => {
  const namespace = randomBytes(16).toString('hex');
  const nonce = randomBytes(16).toString('hex');
  const runtime = `run_${'0'.repeat(26)}`;
  const pipe = `\\\\.\\pipe\\Aura\\${namespace}\\${runtime}\\${nonce}`;
  const sockets: Socket[] = [];
  const server = createServer((socket) => {
    sockets.push(socket);
    socket.on('error', () => undefined);
    socket.pause();
  });
  let connection: Awaited<ReturnType<typeof connectMainPipe>> | undefined;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  try {
    await new Promise<void>((resolveReady, reject) => {
      server.once('error', reject);
      server.listen(pipe, resolveReady);
    });
    connection = await connectMainPipe({
      addonPath: resolve(
        import.meta.dirname,
        '../native/aura-main-pipe/build/Release/aura_main_pipe.node',
      ),
      namespaceObjectHash: namespace,
      pipeNonce: nonce,
      runtimeInstanceId: runtime,
      expectedServerPid: process.pid,
    });
    const outcomes: string[] = [];
    const observe = (name: string, work: Promise<unknown>): Promise<string> =>
      work.then(
        () => {
          outcomes.push(`${name}:resolved`);
          return `${name}:resolved`;
        },
        (error: { code?: string }) => {
          const result = `${name}:rejected:${String(error.code)}`;
          outcomes.push(result);
          return result;
        },
      );
    const reading = observe('read', connection.read(1024));
    const writing = observe('write', connection.write(Buffer.alloc(4 * 1024 * 1024, 2)));
    await new Promise<void>((resolveTurn) => setTimeout(resolveTurn, 100));
    expect(outcomes, 'both operations must still be inside the real pipe').toEqual([]);

    const startedClosing = Date.now();
    connection.close();
    expect(Date.now() - startedClosing).toBeLessThan(2_000);
    // Keep Main busy while kernel cancellation completes. A normal close must retain
    // the queued JS completions instead of treating this live environment as torn down.
    const busyUntil = Date.now() + 100;
    while (Date.now() < busyUntil) {
      /* synchronous Main work */
    }
    const result = await Promise.race([
      Promise.all([reading, writing]),
      new Promise<'still-pending'>((resolveTimeout) => {
        timeout = setTimeout(() => resolveTimeout('still-pending'), 2_000);
      }),
    ]);
    expect(result).toEqual([
      'read:rejected:ERR_AURA_PIPE_CANCELLED',
      'write:rejected:ERR_AURA_PIPE_CANCELLED',
    ]);
  } finally {
    if (timeout !== undefined) clearTimeout(timeout);
    connection?.close();
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolveClosed) => server.close(() => resolveClosed()));
  }
}, 10_000);

it('L03 B0 finalizes pending native read and write without a main-thread callback', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aura-native-finalizer-'));
  roots.push(root);
  const namespace = randomBytes(16).toString('hex');
  const nonce = randomBytes(16).toString('hex');
  const runtime = `run_${'0'.repeat(26)}`;
  const pipe = `\\\\.\\pipe\\Aura\\${namespace}\\${runtime}\\${nonce}`;
  const sockets: Socket[] = [];
  const server = createServer((socket) => {
    sockets.push(socket);
    socket.pause();
  });
  let child: ChildProcess | undefined;
  try {
    await new Promise<void>((resolveReady, reject) => {
      server.once('error', reject);
      server.listen(pipe, resolveReady);
    });
    const worker = join(root, 'worker.cjs');
    const workerSource = `
      const addon = require(${JSON.stringify(resolve(import.meta.dirname, '../native/aura-main-pipe/build/Release/aura_main_pipe.node'))});
      addon.connect(${JSON.stringify({ namespaceObjectHash: namespace, pipeNonce: nonce, runtimeInstanceId: runtime, expectedServerPid: process.pid })})
        .then(connection => {
          connection.read(1024).catch(() => {});
          connection.write(Buffer.alloc(4 * 1024 * 1024, 2)).catch(() => {});
          setTimeout(() => require('node:worker_threads').parentPort.postMessage('pending-io'), 100);
        }).catch(error => { console.error(error); process.exit(2); });
    `;
    await writeFile(
      worker,
      `
      const { Worker } = require('node:worker_threads');
      const worker = new Worker(${JSON.stringify(workerSource)}, { eval: true });
      worker.on('error', error => { console.error(error); process.exit(2); });
      worker.once('message', message => {
        process.stdout.write(message + '\\n');
        worker.terminate().then(() => process.exit(0));
      });
    `,
    );
    child = spawn(process.execPath, [worker], { cwd: root, stdio: ['ignore', 'pipe', 'pipe'] });
    let output = '';
    let errors = '';
    child.stdout?.on('data', (bytes: Buffer) => {
      output += bytes.toString();
    });
    child.stderr?.on('data', (bytes: Buffer) => {
      errors += bytes.toString();
    });
    const exit = await Promise.race([
      new Promise<number | null>((resolveExit, reject) => {
        child!.once('exit', resolveExit);
        child!.once('error', reject);
      }),
      new Promise<'timeout'>((resolveTimeout) =>
        setTimeout(() => resolveTimeout('timeout'), 5_000),
      ),
    ]);
    expect(errors).toBe('');
    expect(output).toContain('pending-io');
    expect(exit, 'native finalizer must drain kernel I/O without processing JS callbacks').toBe(0);
  } finally {
    if (child?.exitCode === null) child.kill();
    for (const socket of sockets) socket.destroy();
    await new Promise<void>((resolveClosed) => server.close(() => resolveClosed()));
  }
}, 15_000);
