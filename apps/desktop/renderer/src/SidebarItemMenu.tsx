import { useId, useLayoutEffect, useRef, useState } from 'react';
import { AppIcon } from './app-icons';
import { copyText } from './clipboard';
import { ModalDialog } from './ModalDialog';
import { useDismissablePopover } from './useDismissablePopover';
import { userMessage } from './agent-session/user-message';
import type { SessionNavigationItem, WorkspaceNavigationItem } from './app-shell-types';
import { removeComposerDraft } from './agent-session/composer-drafts';
import { archiveWorkspaceById } from './session-lifecycle/workspace-lifecycle';

export function SidebarItemMenu({
  workspace,
  session,
}: {
  workspace: WorkspaceNavigationItem;
  session?: SessionNavigationItem;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useDismissablePopover(ref);
  const menuRef = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const menu = menuRef.current;
    const trigger = ref.current?.querySelector('button');
    if (!open || !menu || !trigger) return;
    // The top layer prevents the scrollable workspace tree from clipping the menu.
    if (typeof menu.showPopover === 'function') menu.showPopover();
    else menu.removeAttribute('popover');
    const anchor = trigger.getBoundingClientRect();
    const bounds = menu.getBoundingClientRect();
    menu.style.left = `${Math.max(8, Math.min(anchor.right - bounds.width, window.innerWidth - bounds.width - 8))}px`;
    menu.style.top = `${Math.max(8, anchor.bottom + bounds.height + 8 > window.innerHeight ? anchor.top - bounds.height - 5 : anchor.bottom + 5)}px`;
    menu.querySelector<HTMLButtonElement>('button')?.focus();
    const closeOnMove = () => setOpen(false);
    // Only a scroll that moves the button (one of its ancestors scrolled) closes the menu: the
    // conversation scrolling beside the sidebar during a turn does not.
    const closeOnScroll = (event: Event) => {
      if (event.target instanceof Node && event.target.contains(trigger)) setOpen(false);
    };
    window.addEventListener('resize', closeOnMove);
    window.addEventListener('scroll', closeOnScroll, true);
    return () => {
      window.removeEventListener('resize', closeOnMove);
      window.removeEventListener('scroll', closeOnScroll, true);
    };
  }, [open, setOpen]);
  const [dialog, setDialog] = useState<'rename' | 'workspace' | 'discard'>();
  const [title, setTitle] = useState('');
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [archiving, setArchiving] = useState(false);
  const titleId = useId();
  const local = session?.id.startsWith('draft_');
  const close = () => {
    if (!busy) setDialog(undefined);
  };
  const rename = async () => {
    if (!session || !title.trim() || busy) return;
    setBusy(true);
    setError(undefined);
    try {
      const result = await window.aura.v1.renameSession({
        session_id: session.id,
        expected_title: session.title,
        title: title.trim(),
      });
      if (!result.ok) throw new Error(userMessage(result.error.message));
      setDialog(undefined);
    } catch (reason) {
      setError(userMessage(reason, '重命名未确认，请刷新后查看。'));
    } finally {
      setBusy(false);
    }
  };
  // Archiving is reversible in 设置 → 归档, so it runs at once; one request at a time. A failure
  // opens the menu again with its reason, so it is seen and can be tried again.
  const archive = async () => {
    if (archiving) return;
    setArchiving(true);
    setError(undefined);
    try {
      if (session) {
        const result = await window.aura.v1.archiveSession({ session_id: session.id });
        if (!result.ok) throw result.error;
      } else await archiveWorkspaceById(workspace.id);
      setOpen(false);
    } catch (reason) {
      setError(userMessage(reason, '归档没有完成，请刷新后查看。'));
      setOpen(true);
    } finally {
      setArchiving(false);
    }
  };
  return (
    <div className="sidebar-item-menu" ref={ref}>
      <button
        type="button"
        className="icon-button"
        aria-label={`${session?.title || (session ? '新会话' : workspace.name)} 更多操作`}
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen(!open)}
      >
        <AppIcon name="more" size={16} />
      </button>
      {open && (
        <div
          className="sidebar-popover"
          role="menu"
          popover="manual"
          ref={menuRef}
          onKeyDown={(event) => {
            if (event.key === 'Escape') {
              setOpen(false);
              ref.current?.querySelector('button')?.focus();
            }
            if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
            event.preventDefault();
            const items = [
              ...event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="menuitem"]'),
            ];
            const index = items.indexOf(document.activeElement as HTMLButtonElement);
            items[
              (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
            ]?.focus();
          }}
        >
          {session ? (
            <button
              type="button"
              role="menuitem"
              onClick={() => {
                setOpen(false);
                setError(undefined);
                setTitle(session.title);
                setDialog(local ? 'discard' : 'rename');
              }}
            >
              {local ? '删除草稿' : '重命名会话'}
            </button>
          ) : null}
          {/* The menu archives; restoring and deleting are in 设置 → 归档. */}
          {session && !local ? (
            <button
              type="button"
              role="menuitem"
              title="归档后可在“设置 → 归档”里恢复或删除"
              disabled={archiving}
              onClick={() => void archive()}
            >
              {archiving ? '正在归档…' : '归档会话'}
            </button>
          ) : null}
          {session ? null : (
            <>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  setOpen(false);
                  setError(undefined);
                  setDialog('workspace');
                }}
              >
                工作区信息
              </button>
              <button
                type="button"
                role="menuitem"
                onClick={() => {
                  void copyText(workspace.root, '复制失败，请在工作区信息中选取路径。').then(
                    (refused) => (refused ? setError(refused) : setOpen(false)),
                  );
                }}
              >
                复制路径
              </button>
              <button
                type="button"
                role="menuitem"
                title="归档后可在“设置 → 归档”里恢复或删除"
                disabled={archiving}
                onClick={() => void archive()}
              >
                {archiving ? '正在归档…' : '归档工作区'}
              </button>
            </>
          )}
          {error && <p role="alert">{error}</p>}
        </div>
      )}
      {(dialog === 'rename' || dialog === 'workspace' || dialog === 'discard') && (
        <ModalDialog className="sidebar-edit-dialog" titleId={titleId} onClose={close}>
          <header className="modal-header">
            <h2 id={titleId}>
              {dialog === 'rename'
                ? '重命名会话'
                : dialog === 'discard'
                  ? '删除这份草稿？'
                  : workspace.name}
            </h2>
            <button
              type="button"
              className="icon-button"
              aria-label="关闭"
              disabled={busy}
              onClick={close}
            >
              <AppIcon name="close" />
            </button>
          </header>
          {dialog === 'rename' ? (
            <form
              className="sidebar-dialog-body"
              onSubmit={(event) => {
                event.preventDefault();
                void rename();
              }}
            >
              <label>
                会话名称
                <input
                  value={title}
                  maxLength={200}
                  onChange={(event) => setTitle(event.target.value)}
                />
              </label>
              {error && <p role="alert">{error}</p>}
              <div className="sidebar-dialog-actions">
                <button type="button" onClick={close} disabled={busy}>
                  取消
                </button>
                <button type="submit" disabled={busy || !title.trim()}>
                  {busy ? '正在保存…' : '保存'}
                </button>
              </div>
            </form>
          ) : dialog === 'discard' ? (
            <div className="sidebar-dialog-body">
              <p>草稿中的未发送文字将被删除。</p>
              <div className="sidebar-dialog-actions">
                <button type="button" onClick={close}>
                  保留
                </button>
                <button
                  type="button"
                  onClick={() => {
                    removeComposerDraft(session!.id);
                    close();
                  }}
                >
                  删除草稿
                </button>
              </div>
            </div>
          ) : (
            <div className="sidebar-dialog-body">
              <span className="sidebar-field-label">本地文件夹</span>
              <p className="workspace-directory-value">{workspace.root}</p>
            </div>
          )}
        </ModalDialog>
      )}
    </div>
  );
}
