// NATIVE LAYER EVIDENCE - not a fake sink.
//
// This case runs the REAL `aura-main-pipe.node` against a REAL Windows named pipe created by this batch,
// in a child process this batch starts. It answers the one question a controlled JavaScript sink cannot:
// what really happens to a write that is stuck inside the OS when the connection is cancelled and closed.
//
// It is deliberately narrow. It does NOT exercise the Core protocol, authentication or the handshake;
// the transport-level behaviour is covered by the controlled-sink cases in `outbound-send-boundary.test.ts`.
import { spawn, type ChildProcess } from 'node:child_process';
import { existsSync } from 'node:fs';
import { Buffer } from 'node:buffer';
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { afterEach, describe, expect, it } from 'vitest';

import { connectMainPipe, type MainPipeConnection } from './main-pipe-bridge.js';

const ADDON_PATH = resolve(
  dirname(fileURLToPath(import.meta.url)),
  '../native/aura-main-pipe/build/Release/aura_main_pipe.node',
);

/**
 * The pipe path the real addon builds: `\\.\pipe\Aura\<namespace hash>\<runtime instance>\<nonce>`.
 *
 * The child server has to create EXACTLY this name, which is what makes this real named-pipe evidence
 * rather than a mock: the addon validates the hash as 32 lowercase hex characters and the runtime instance
 * as `run_` plus 26 Crockford characters.
 */
const namespaceObjectHash = 'a'.repeat(32);
const pipeNonce = 'b'.repeat(32);
const runtimeInstanceId = `run_${'0123456789ABCDEFGHJKMNPQRS'.slice(0, 26)}`;
const pipePath = `\\\\.\\pipe\\Aura\\${namespaceObjectHash}\\${runtimeInstanceId}\\${pipeNonce}`;

let child: ChildProcess | undefined;

afterEach(() => {
  child?.kill();
  child = undefined;
});

/** Start a real named-pipe server that accepts ONE connection and then never reads from it. */
async function startSilentServer(): Promise<number> {
  const script = `
    const net = require('node:net');
    const path = ${JSON.stringify(pipePath)};
    const server = net.createServer((socket) => {
      // Accepted, and deliberately never read: a client that keeps writing blocks inside the OS, which
      // is exactly the state this case has to observe.
      socket.on('error', () => undefined);
      process.stdout.write('accepted\\n');
    });
    server.on('error', (error) => {
      process.stdout.write('error:' + String(error) + '\\n');
      process.exit(1);
    });
    server.listen(path, () => process.stdout.write('ready\\n'));
  `;
  const server = spawn(process.execPath, ['-e', script], {
    stdio: ['ignore', 'pipe', 'pipe'],
    windowsHide: true,
  });
  child = server;
  const pid = server.pid;
  if (pid === undefined) throw new Error('the pipe server child did not start');
  if (server.stdout === null) throw new Error('the pipe server child has no stdout');
  await new Promise<void>((resolveReady, rejectReady) => {
    const timer = setTimeout(
      () => rejectReady(new Error('the pipe server did not start in time')),
      10_000,
    );
    let buffered = '';
    server.stdout.on('data', (chunk: Buffer) => {
      buffered += chunk.toString();
      if (buffered.includes('ready')) {
        clearTimeout(timer);
        resolveReady();
      }
    });
    server.on('error', rejectReady);
  });
  return pid;
}

describe('M1 native teardown (real addon, real named pipe)', () => {
  it('settles a stalled native write and does not wedge close', async () => {
    expect(
      existsSync(ADDON_PATH),
      'the native bridge has to be built for this layer to be observable',
    ).toBe(true);
    const serverPid = await startSilentServer();
    const connection: MainPipeConnection = await connectMainPipe({
      addonPath: ADDON_PATH,
      expectedServerPid: serverPid,
      namespaceObjectHash,
      pipeNonce,
      runtimeInstanceId,
    });

    // 1. A small write really does reach the pipe.
    await connection.write(Buffer.alloc(64, 0x1));

    // 2. A write larger than the pipe buffer cannot complete while the server never reads. The addon owns
    //    its own COPY of the bytes, which is what makes it safe for the caller to stop caring about them.
    const stalled = connection.write(Buffer.alloc(4 * 1024 * 1024, 0x2));
    let stalledSettled = false;
    void stalled.then(
      () => {
        stalledSettled = true;
      },
      () => {
        stalledSettled = true;
      },
    );
    await new Promise((resolveTurn) => setTimeout(resolveTurn, 250));
    expect(stalledSettled).toBe(false);

    // 3. Cancelling reaches the OS, and the pending write really settles: there is a finite, settleable
    //    holder for it instead of a promise nobody can end.
    const cancelled = connection.cancel();
    expect(typeof cancelled).toBe('boolean');
    const outcome = await Promise.race([
      stalled.then(
        () => 'resolved',
        (error: Error) => `rejected:${String((error as { code?: string }).code)}`,
      ),
      new Promise<string>((resolveRace) => {
        setTimeout(() => resolveRace('still-pending'), 5_000);
      }),
    ]);
    expect(outcome).not.toBe('still-pending');
    expect(outcome).toBe('rejected:ERR_AURA_PIPE_CANCELLED');

    // 4. And closing does not wedge Main: it returns, and the native thread tears down behind it.
    const startedClosing = Date.now();
    connection.close();
    expect(Date.now() - startedClosing).toBeLessThan(2_000);
    await new Promise((resolveTurn) => setTimeout(resolveTurn, 100));
  }, 30_000);
});
