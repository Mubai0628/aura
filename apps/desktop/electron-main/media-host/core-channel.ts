interface Handler {
  command(value: unknown, runtime: string): void;
  cancel(value: unknown, runtime: string): void;
}
const handlers = new WeakMap<object, Handler>();
export function registerMediaCommands(owner: object, handler: Handler): void {
  handlers.set(owner, handler);
}
export function mediaCommandNotification(
  owner: object,
  method: string,
  params: unknown,
  runtime: string,
): boolean {
  if (method === 'media.command.v1') {
    handlers.get(owner)?.command(params, runtime);
    return true;
  }
  if (method === 'media.cancel.v1') {
    handlers.get(owner)?.cancel(params, runtime);
    return true;
  }
  return false;
}
