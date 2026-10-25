import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, test, vi } from 'vitest';
import { useRef, useState } from 'react';

import { ConfirmationDialog } from './ConfirmationDialog';
import { ModalDialog } from './ModalDialog';

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  document.querySelector('#root')?.remove();
});

describe('ConfirmationDialog', () => {
  test('explicit safe focus wins over the native dialog initial focus', () => {
    vi.spyOn(HTMLDialogElement.prototype, 'showModal').mockImplementation(function (
      this: HTMLDialogElement,
    ) {
      this.open = true;
      this.querySelector<HTMLButtonElement>('button')?.focus();
    });
    render(
      <ConfirmationDialog
        busy={false}
        confirmLabel="重启"
        description="运行中的任务会被中断。"
        onCancel={vi.fn()}
        onConfirm={vi.fn()}
        title="确认重启"
        titleId="restart-focus-title"
      />,
    );
    expect(document.activeElement).toBe(screen.getByRole('button', { name: '取消' }));
  });

  test('inerts the application and restores focus to the trigger after cancel', async () => {
    const user = userEvent.setup();
    const applicationRoot = document.createElement('div');
    applicationRoot.id = 'root';
    document.body.append(applicationRoot);

    function Fixture() {
      const [open, setOpen] = useState(false);
      const triggerRef = useRef<HTMLButtonElement>(null);
      return (
        <>
          <button onClick={() => setOpen(true)} ref={triggerRef} type="button">
            删除连接
          </button>
          {open ? (
            <ConfirmationDialog
              busy={false}
              confirmLabel="删除"
              description="此操作不可撤销。"
              onCancel={() => setOpen(false)}
              onConfirm={vi.fn()}
              returnFocusRef={triggerRef}
              title="确认删除"
              titleId="confirmation-title"
            />
          ) : null}
        </>
      );
    }

    render(<Fixture />, { container: applicationRoot });
    const trigger = screen.getByRole('button', { name: '删除连接' });
    await user.click(trigger);
    expect(applicationRoot.inert).toBe(true);
    expect(applicationRoot.getAttribute('aria-hidden')).toBe('true');
    expect(document.activeElement).toBe(screen.getByRole('button', { name: '取消' }));

    await user.click(screen.getByRole('button', { name: '取消' }));
    expect(applicationRoot.inert).not.toBe(true);
    expect(applicationRoot.hasAttribute('aria-hidden')).toBe(false);
    expect(document.activeElement).toBe(trigger);
  });

  test('only the confirmation layer handles Escape while the base modal is suspended', async () => {
    const user = userEvent.setup();
    const closeBase = vi.fn();

    function Fixture() {
      const [confirming, setConfirming] = useState(true);
      return (
        <>
          <ModalDialog onClose={closeBase} suspended={confirming} titleId="base-title">
            <h2 id="base-title">基础弹窗</h2>
            <button type="button">基础操作</button>
          </ModalDialog>
          {confirming ? (
            <ConfirmationDialog
              busy={false}
              confirmLabel="确认"
              description="确认当前操作。"
              onCancel={() => setConfirming(false)}
              onConfirm={vi.fn()}
              title="二次确认"
              titleId="confirm-title"
            />
          ) : null}
        </>
      );
    }

    render(<Fixture />);
    await user.keyboard('{Escape}');
    expect(closeBase).not.toHaveBeenCalled();
    expect(screen.getByRole('dialog', { name: '基础弹窗' })).toBeTruthy();
    expect(screen.queryByRole('dialog', { name: '二次确认' })).toBeNull();
  });
});
