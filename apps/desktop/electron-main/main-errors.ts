import type { EventEmitter } from 'node:events';
import type { MainErrorKind, MainErrorNoticeV1 } from '../shared/main-errors-api.js';

interface ErrorPorts {
  log: (event: string, fields: Record<string, unknown>) => void;
  logPath: () => string;
  publish: (notice: MainErrorNoticeV1) => void;
  exit: () => void;
  fallback: (message: string) => void;
}
function describe(reason: unknown): { message: string; stack?: string } {
  try {
    return reason instanceof Error
      ? {
          message: String(reason.message).slice(0, 16384),
          stack: String(reason.stack ?? '').slice(0, 32768),
        }
      : { message: String(reason).slice(0, 16384) };
  } catch {
    return { message: 'Unable to inspect uncaught error' };
  }
}
/** An error must neither recurse through its logger nor hold quit behind a modal. */
export class MainErrorReporter {
  private quitting = false;
  private sequence = 0;
  private latest: MainErrorNoticeV1 | null = null;
  constructor(private readonly ports: ErrorPorts) {}
  beginQuit() {
    this.quitting = true;
  }
  isQuitting() {
    return this.quitting;
  }
  snapshot(): MainErrorNoticeV1 | null {
    return this.latest && { ...this.latest };
  }
  report(kind: MainErrorKind, reason: unknown) {
    const fields = { ...describe(reason), quitting: this.quitting };
    let logged = false;
    let logPath = 'stderr';
    try {
      logPath = this.ports.logPath();
      this.ports.log(`main.${kind}`, fields);
      logged = true;
    } catch (error) {
      this.fallback(
        JSON.stringify({ event: `main.${kind}`, fields, logging_error: describe(error).message }),
      );
    }
    if (this.quitting) {
      try {
        this.ports.exit();
      } catch (error) {
        this.fallback(`Main exit failed: ${describe(error).message}`);
      }
      return;
    }
    const notice: MainErrorNoticeV1 = {
      version: 1,
      id: `main_error_${++this.sequence}`,
      kind,
      logged,
      log_path: logPath,
    };
    this.latest = notice;
    try {
      this.ports.publish({ ...notice });
    } catch (error) {
      this.fallback(`Main error notice delivery failed: ${describe(error).message}`);
    }
  }
  private fallback(message: string) {
    try {
      this.ports.fallback(message);
    } catch {
      /* No further error channel remains. */
    }
  }
}
export function installMainErrorHandlers(
  processEvents: EventEmitter,
  appEvents: EventEmitter,
  reporter: MainErrorReporter,
) {
  const exception = (reason: unknown) => reporter.report('uncaught_exception', reason);
  const rejection = (reason: unknown) => reporter.report('unhandled_rejection', reason);
  const quitting = () => reporter.beginQuit();
  // Mark quit before any existing listener can fail. The default Electron error
  // dialog yields to the explicit process listener installed here.
  appEvents.prependListener('before-quit', quitting);
  processEvents.on('uncaughtException', exception);
  processEvents.on('unhandledRejection', rejection);
  return () => {
    appEvents.removeListener('before-quit', quitting);
    processEvents.removeListener('uncaughtException', exception);
    processEvents.removeListener('unhandledRejection', rejection);
  };
}
