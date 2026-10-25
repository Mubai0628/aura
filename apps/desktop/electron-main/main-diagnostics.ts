import { app, type IpcMainInvokeEvent, type WebContents } from 'electron';
import { writeSync } from 'node:fs';
import { MAIN_ERROR_EVENT_CHANNEL } from '../shared/main-errors-api.js';
import { createMainLogger } from './main-log.js';
import { MainErrorReporter, installMainErrorHandlers } from './main-errors.js';
import { registerMainErrorsIpc } from './main-errors-ipc.js';
import { createMainQuitHandler } from './main-shutdown.js';
import { publishToRenderer } from './publish-to-renderer.js';

/** Own Main's logging and error/quit lifecycle; the entry point only supplies live resources. */
export function installMainDiagnostics(options: {
  localAppData: string;
  namespace: () => string | undefined;
  contents: () => WebContents | undefined;
  trust: (event: IpcMainInvokeEvent) => void;
  stop: () => void;
  flush: () => Promise<unknown> | undefined;
  /** Set when Aura should exit with a code of its own once Core has stopped. */
  exitCode?: () => number | undefined;
}) {
  const logger = createMainLogger(options.localAppData, options.namespace);
  const reporter = new MainErrorReporter({
    log: logger.appendSync,
    logPath: logger.path,
    publish: (notice) => publishToRenderer(options.contents(), MAIN_ERROR_EVENT_CHANNEL, notice),
    fallback: (message) => {
      writeSync(2, `${message}\n`);
    },
    exit: () => {
      try {
        options.stop();
      } finally {
        try {
          app.exit(1);
        } catch {
          process.exit(1);
        }
      }
    },
  });
  installMainErrorHandlers(process, app, reporter);
  registerMainErrorsIpc(reporter, options.trust);
  app.on(
    'before-quit',
    createMainQuitHandler({
      stop: options.stop,
      flush: options.flush,
      quit: () => {
        const code = options.exitCode?.();
        if (code === undefined) app.quit();
        else app.exit(code);
      },
      report: (kind, error) => reporter.report(kind, error),
    }),
  );
  return { append: logger.append, isQuitting: () => reporter.isQuitting() };
}
