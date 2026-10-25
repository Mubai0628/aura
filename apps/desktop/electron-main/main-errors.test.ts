// @vitest-environment node
import { EventEmitter } from 'node:events';
import { mkdtemp, readFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, test, vi } from 'vitest';
import { MainErrorReporter, installMainErrorHandlers } from './main-errors.js';
import { createMainLogger } from './main-log.js';
import { createMainQuitHandler } from './main-shutdown.js';

function fixture() {
  const events: string[] = [];
  const ports = {
    log: vi.fn<(event: string, fields: Record<string, unknown>) => void>(() => {
      events.push('logged');
    }),
    logPath: () => 'C:/temporary-Aura/logs/main.jsonl',
    publish: vi.fn(() => {
      events.push('published');
    }),
    exit: vi.fn(() => {
      events.push('exit');
    }),
    fallback: vi.fn<(message: string) => void>(),
  };
  return { ports, events, reporter: new MainErrorReporter(ports) };
}
test('regular exceptions and rejections are logged, published safely and retained for a late renderer', () => {
  const { ports, events, reporter } = fixture();
  expect(reporter.snapshot()).toBeNull();
  reporter.report('uncaught_exception', new Error('temporary secret-like test value'));
  const notice = reporter.snapshot()!;
  expect(notice).toMatchObject({ id: 'main_error_1', logged: true });
  expect(JSON.stringify(notice)).not.toContain('secret-like');
  notice.log_path = 'modified by a consumer';
  expect(reporter.snapshot()!.log_path).toBe(ports.logPath());
  reporter.report('unhandled_rejection', 'temporary rejection');
  expect(reporter.snapshot()!.id).toBe('main_error_2');
  expect(ports.log).toHaveBeenLastCalledWith('main.unhandled_rejection', {
    message: 'temporary rejection',
    quitting: false,
  });
  expect(events).toEqual(['logged', 'published', 'logged', 'published']);
  expect(ports.exit).not.toHaveBeenCalled();
});
test('quitting errors synchronously log before exiting and never publish to a destroyed window', () => {
  const { ports, events, reporter } = fixture();
  reporter.beginQuit();
  reporter.report('uncaught_exception', new Error('temporary shutdown error'));
  expect(events).toEqual(['logged', 'exit']);
  expect(ports.log.mock.calls[0]).toMatchObject(['main.uncaught_exception', { quitting: true }]);
  expect(ports.publish).not.toHaveBeenCalled();
});
test('disk and publication errors have a nonrecursive fallback; even an uninspectable reason cannot block quit', () => {
  const { ports, reporter } = fixture();
  ports.log.mockImplementation(() => {
    throw new Error('temporary disk full');
  });
  ports.publish.mockImplementation(() => {
    throw new Error('Object has been destroyed');
  });
  reporter.report('unhandled_rejection', new Error('original temporary failure'));
  expect(reporter.snapshot()!.logged).toBe(false);
  expect(ports.fallback).toHaveBeenCalledTimes(2);
  expect(ports.fallback.mock.calls[0]?.[0]).toContain('original temporary failure');
  expect(ports.log).toHaveBeenCalledTimes(1);
  ports.fallback.mockImplementation(() => {
    throw new Error('closed stderr');
  });
  reporter.beginQuit();
  const reason = {
    toString() {
      throw new Error('uninspectable');
    },
  };
  expect(() => reporter.report('uncaught_exception', reason)).not.toThrow();
  expect(ports.exit).toHaveBeenCalledOnce();
});
test('process listeners mark quit before existing app listeners and release their ownership', () => {
  const { ports, reporter } = fixture();
  const processEvents = new EventEmitter(),
    appEvents = new EventEmitter();
  appEvents.on('before-quit', () =>
    processEvents.emit('uncaughtException', new Error('quit listener failed')),
  );
  const dispose = installMainErrorHandlers(processEvents, appEvents, reporter);
  processEvents.emit('unhandledRejection', 'temporary rejection');
  expect(ports.publish).toHaveBeenCalledOnce();
  appEvents.emit('before-quit');
  expect(ports.exit).toHaveBeenCalledOnce();
  expect(ports.log.mock.calls.at(-1)).toMatchObject([
    'main.uncaught_exception',
    { quitting: true },
  ]);
  dispose();
  expect(processEvents.listenerCount('uncaughtException')).toBe(0);
  expect(processEvents.listenerCount('unhandledRejection')).toBe(0);
  expect(appEvents.listenerCount('before-quit')).toBe(1);
});
test('real Main logs survive a namespace transition and synchronous shutdown recording', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aura-main-logging-'));
  let namespace: string | undefined;
  const logger = createMainLogger(root, () => namespace);
  try {
    const startup = logger.path();
    logger.appendSync('main.uncaught_exception', { message: 'temporary startup' });
    expect(JSON.parse((await readFile(startup, 'utf8')).trim()).event).toBe(
      'main.uncaught_exception',
    );
    namespace = 'temporary-namespace';
    await logger.append('core.binding', { state: 'temporary fixture' });
    logger.appendSync('main.unhandled_rejection', { quitting: true });
    const records = (await readFile(logger.path(), 'utf8'))
      .trim()
      .split('\n')
      .map((line) => JSON.parse(line));
    expect(records.map((record) => record.event)).toEqual([
      'core.binding',
      'main.unhandled_rejection',
    ]);
    expect(records[1].component).toBe('electron_main');
    expect(records[1].fields.quitting).toBe(true);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
test('quit stops once, waits for flush and permits the second native quit', async () => {
  let finish: (() => void) | undefined;
  const ports = {
    stop: vi.fn(),
    flush: () =>
      new Promise<void>((resolve) => {
        finish = resolve;
      }),
    quit: vi.fn(),
    report: vi.fn(),
  };
  const quit = createMainQuitHandler(ports);
  const preventDefault = vi.fn();
  quit({ preventDefault });
  await Promise.resolve();
  expect(ports.quit).not.toHaveBeenCalled();
  finish!();
  await vi.waitFor(() => expect(ports.quit).toHaveBeenCalledOnce());
  quit({ preventDefault });
  expect(preventDefault).toHaveBeenCalledOnce();
  expect(ports.stop).toHaveBeenCalledOnce();
});
test.each(['stop', 'flush', 'quit'] as const)(
  'quit reports %s failure instead of leaving a rejected cleanup promise',
  async (stage) => {
    const error = new Error('temporary cleanup failure');
    const ports = {
      stop: vi.fn(),
      flush: vi.fn().mockResolvedValue(undefined),
      quit: vi.fn(),
      report: vi.fn(),
    };
    if (stage === 'stop')
      ports.stop.mockImplementation(() => {
        throw error;
      });
    else if (stage === 'flush') ports.flush.mockRejectedValue(error);
    else
      ports.quit.mockImplementation(() => {
        throw error;
      });
    createMainQuitHandler(ports)({ preventDefault: vi.fn() });
    await vi.waitFor(() =>
      expect(ports.report).toHaveBeenCalledWith(
        stage === 'stop' ? 'uncaught_exception' : 'unhandled_rejection',
        error,
      ),
    );
    expect(ports.quit).toHaveBeenCalledTimes(stage === 'quit' ? 1 : 0);
  },
);
