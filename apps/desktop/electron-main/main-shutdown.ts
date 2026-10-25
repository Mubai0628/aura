interface QuitEvent {
  preventDefault(): void;
}
interface QuitPorts {
  stop(): void;
  flush(): Promise<unknown> | undefined;
  quit(): void;
  report(kind: 'uncaught_exception' | 'unhandled_rejection', error: unknown): void;
}
/** Stop once, await window-state writes, then allow the second before-quit through. */
export function createMainQuitHandler(ports: QuitPorts) {
  let stopping = false;
  return (event: QuitEvent) => {
    if (stopping) return;
    event.preventDefault();
    stopping = true;
    try {
      ports.stop();
    } catch (error) {
      ports.report('uncaught_exception', error);
      return;
    }
    void Promise.resolve()
      .then(() => ports.flush())
      .then(() => ports.quit())
      .catch((error) => ports.report('unhandled_rejection', error));
  };
}
