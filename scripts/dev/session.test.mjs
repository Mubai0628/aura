import assert from 'node:assert/strict';
import { EventEmitter } from 'node:events';
import test from 'node:test';
import { runDevSession } from './session.mjs';

// Development Aura is stopped with Ctrl+C or by closing the terminal. That
// used to force-kill Electron and Core together, so Core never recorded a normal exit and every
// start checked the whole Store. A Ctrl+C, a closed terminal or a closed window now ends Aura the
// way the window's × does.

function child(pid) {
  const process = new EventEmitter();
  process.pid = pid;
  process.exitCode = null;
  process.end = (code) => {
    process.exitCode = code;
    process.emit('exit', code);
  };
  return process;
}

function session({ windows = 1, electronEndsOnClose = true } = {}) {
  const calls = [];
  const renderer = child(10);
  const electron = child(20);
  const signals = new EventEmitter();
  let exitedWith;
  const ended = new Promise((resolve) => {
    runDevSession({
      spawnRenderer: () => renderer,
      spawnElectron: () => electron,
      closeAuraWindows: () => {
        calls.push('close windows');
        if (windows && electronEndsOnClose) setImmediate(() => electron.end(0));
        return windows;
      },
      killTree: (pid) => {
        calls.push(`kill ${pid}`);
        if (pid === electron.pid && electron.exitCode === null) electron.end(1);
      },
      onExit: (code) => {
        exitedWith = code;
        resolve();
      },
      signals,
      quitTimeoutMs: 50,
    });
  });
  return { calls, renderer, electron, signals, ended, exit: () => exitedWith };
}

test('Ctrl+C closes Aura the normal way, then stops the renderer, and never forces Electron', async () => {
  const run = session();
  run.signals.emit('SIGINT');
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10']);
  assert.equal(run.exit(), 130);
});

test('closing the terminal does the same', async () => {
  const run = session();
  run.signals.emit('SIGHUP');
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10']);
});

test('an Electron that does not end in time is ended by force', async () => {
  const run = session({ electronEndsOnClose: false });
  run.signals.emit('SIGINT');
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10', 'kill 20']);
});

test('before the window exists (still building) the launcher is ended at once', async () => {
  const run = session({ windows: 0 });
  run.signals.emit('SIGINT');
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10', 'kill 20']);
});

test('closing the window ends the session with Electron, and the renderer with it', async () => {
  const run = session();
  run.electron.end(0);
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10']);
  assert.equal(run.exit(), 0);
});

test('a renderer that stops closes Aura the normal way and fails the session', async () => {
  const run = session();
  run.renderer.end(1);
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10']);
  assert.equal(run.exit(), 1);
});

test('a second Ctrl+C while stopping changes nothing', async () => {
  const run = session({ electronEndsOnClose: false });
  run.signals.emit('SIGINT');
  run.signals.emit('SIGINT');
  await run.ended;
  assert.deepEqual(run.calls, ['close windows', 'kill 10', 'kill 20']);
});
