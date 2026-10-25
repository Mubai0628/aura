import type { ChildProcess } from 'node:child_process';

/** Observe the OS child exit, not ChildProcess.killed (only a sent signal). */
export function waitForChildExit(child: ChildProcess, timeoutMs = 30_000): Promise<void> {
  if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((resolve, reject) => {
    const clear = () => {
      clearTimeout(deadline);
      child.removeListener('exit', exited);
      child.removeListener('error', failed);
    };
    const exited = () => {
      clear();
      resolve();
    };
    const failed = (error: Error) => {
      clear();
      reject(error);
    };
    const deadline = setTimeout(() => {
      clear();
      reject(new Error('Core process exit was not confirmed before the deadline.'));
    }, timeoutMs);
    child.once('exit', exited);
    child.once('error', failed);
  });
}
