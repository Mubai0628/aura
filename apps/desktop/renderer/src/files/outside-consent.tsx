import { useId, useState, type ReactNode } from 'react';
import { AppIcon } from '../app-icons';
import { ModalDialog } from '../ModalDialog';

/**
 * Files outside the workspace the user said to open from a call's row: by conversation and file,
 * while the window is open. A restart asks again.
 */
const agreed = new Set<string>();
const key = (session: string, path: string) =>
  `${session}|${path.replaceAll('/', '\\').toLowerCase()}`;

/** Tests: every answer forgotten. */
export function forgetOutsideConsentsForTests(): void {
  agreed.clear();
}

/**
 * Asks before the first opening of a file outside the workspace in this conversation: `ask(run)`
 * runs `run` at once when the user already said yes, otherwise after a yes in `dialog`.
 */
export function useOutsideConsent(
  session: string,
  path: string,
): { ask: (run: () => void) => void; dialog: ReactNode } {
  const titleId = useId();
  const [pending, setPending] = useState<(() => void) | undefined>();
  const ask = (run: () => void) => {
    if (agreed.has(key(session, path))) run();
    else setPending(() => run);
  };
  const cancel = () => setPending(undefined);
  const dialog = pending ? (
    <ModalDialog className="sidebar-edit-dialog" onClose={cancel} titleId={titleId}>
      <header className="modal-header">
        <h2 id={titleId}>打开工作区外的文件？</h2>
        <button aria-label="关闭" className="icon-button" onClick={cancel} type="button">
          <AppIcon name="close" />
        </button>
      </header>
      <div className="sidebar-dialog-body">
        <p className="outside-file-path">{path}</p>
        <div className="sidebar-dialog-actions">
          <button onClick={cancel} type="button">
            取消
          </button>
          <button
            onClick={() => {
              agreed.add(key(session, path));
              setPending(undefined);
              pending();
            }}
            type="button"
          >
            打开
          </button>
        </div>
      </div>
    </ModalDialog>
  ) : null;
  return { ask, dialog };
}
