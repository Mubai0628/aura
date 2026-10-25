import { act, cleanup, render, screen, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { BrowserPageState, BrowserUserCommand } from '../../../shared/browser-api';
import { BrowserPage } from './BrowserPage';
import { browserStore } from './browser-store';

const S = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const page = (over: Partial<BrowserPageState> = {}): BrowserPageState => ({
  sessionId: S,
  pageId: 'tab-1',
  url: 'https://example.com/',
  title: 'Example',
  loading: false,
  canGoBack: true,
  canGoForward: false,
  agentActive: false,
  userInControl: false,
  closed: false,
  ...over,
});

afterEach(() => cleanup());

let command: ReturnType<typeof vi.fn<(value: BrowserUserCommand) => Promise<unknown>>>;
beforeEach(() => {
  command = vi.fn(async (value: BrowserUserCommand) =>
    value.kind === 'open' ? { ok: true, state: page({ pageId: value.pageId }) } : { ok: true },
  );
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        browser: {
          command,
          pages: vi.fn(async () => []),
          onState: vi.fn(() => () => undefined),
          getSettings: vi.fn(),
          updateSettings: vi.fn(),
        },
      },
    },
  });
});

const kinds = () => command.mock.calls.map(([value]) => value.kind);

describe('a page of the browser', () => {
  it('opens its page and drives it from the toolbar', async () => {
    const user = userEvent.setup();
    render(<BrowserPage sessionId={S} pageId="tab-1" visible />);
    await waitFor(() =>
      expect((screen.getByLabelText('地址') as HTMLInputElement).value).toBe(
        'https://example.com/',
      ),
    );
    expect(command).toHaveBeenCalledWith({ kind: 'open', sessionId: S, pageId: 'tab-1' });
    expect((screen.getByRole('button', { name: '前进' }) as HTMLButtonElement).disabled).toBe(true);
    await user.click(screen.getByRole('button', { name: '后退' }));
    await user.click(screen.getByRole('button', { name: '刷新' }));
    expect(kinds()).toEqual(expect.arrayContaining(['back', 'reload']));
    const address = screen.getByLabelText('地址');
    await user.clear(address);
    await user.type(address, 'localhost:5173{Enter}');
    expect(command).toHaveBeenCalledWith({
      kind: 'navigate',
      sessionId: S,
      pageId: 'tab-1',
      input: 'localhost:5173',
    });
    // A user page offers no takeover: only the model's page has a controller.
    expect(screen.queryByRole('button', { name: /接管/u })).toBeNull();
  });

  // The page's own DevTools, in a window of their own.
  it("opens the page's DevTools from the toolbar", async () => {
    const user = userEvent.setup();
    render(<BrowserPage sessionId={S} pageId="tab-1" visible />);
    await waitFor(() =>
      expect((screen.getByLabelText('地址') as HTMLInputElement).value).toBe(
        'https://example.com/',
      ),
    );
    await user.click(screen.getByRole('button', { name: '开发者工具' }));
    expect(command).toHaveBeenCalledWith({ kind: 'devtools', sessionId: S, pageId: 'tab-1' });
  });

  it("lets the user take over the model's page and shows who is acting", async () => {
    const user = userEvent.setup();
    render(<BrowserPage sessionId={S} pageId="model" visible />);
    await screen.findByRole('button', { name: '接管' });
    act(() => browserStore.put(page({ pageId: 'model', agentActive: true })));
    expect(screen.getByRole('status').textContent).toContain('Aura 正在操作');
    await user.click(screen.getByRole('button', { name: '接管' }));
    expect(command).toHaveBeenLastCalledWith({
      kind: 'take-control',
      sessionId: S,
      pageId: 'model',
    });
    act(() => browserStore.put(page({ pageId: 'model', userInControl: true })));
    expect(screen.getByRole('button', { name: '交还 Aura' }).getAttribute('aria-pressed')).toBe(
      'true',
    );
    const status = screen.getByRole('status');
    expect(status.textContent).toContain('你在操作');
    // Said above the page, never
    // on the area the page is laid over.
    expect(status.closest('.browser-stage')).toBeNull();
    // The model's request to wait for them leaves the user their handback.
    act(() =>
      browserStore.put(
        page({ pageId: 'model', userInControl: true, assist: { reason: '请操作完', deadline: 1 } }),
      ),
    );
    await user.click(screen.getByRole('button', { name: '交还 Aura' }));
    expect(command).toHaveBeenLastCalledWith({
      kind: 'release-control',
      sessionId: S,
      pageId: 'model',
    });
    // Asked for help without having taken over, they answer the request itself.
    act(() =>
      browserStore.put(page({ pageId: 'model', assist: { reason: '请登录', deadline: 1 } })),
    );
    expect(screen.queryByRole('button', { name: /接管|交还/u })).toBeNull();
  });

  it('shows the start page instead of a blank page, and Home goes back to it', async () => {
    const user = userEvent.setup();
    const area = vi
      .spyOn(HTMLElement.prototype, 'getBoundingClientRect')
      .mockReturnValue(new DOMRect(0, 0, 400, 300));
    command.mockImplementation(async (value: BrowserUserCommand) =>
      value.kind === 'open'
        ? {
            ok: true,
            state: page({ pageId: 'tab-home', url: 'about:blank', title: '', canGoBack: false }),
          }
        : { ok: true },
    );
    render(<BrowserPage sessionId={S} pageId="tab-home" visible />);
    const field = await screen.findByLabelText('打开网址');
    expect((screen.getByLabelText('地址') as HTMLInputElement).value).toBe('');
    expect((screen.getByRole('button', { name: '主页' }) as HTMLButtonElement).disabled).toBe(true);
    // The page itself stays hidden while the start page is shown.
    expect(kinds()).not.toContain('show');
    await user.type(field, 'example.com{Enter}');
    expect(command).toHaveBeenLastCalledWith({
      kind: 'navigate',
      sessionId: S,
      pageId: 'tab-home',
      input: 'example.com',
    });
    act(() => browserStore.put(page({ pageId: 'tab-home' })));
    expect(screen.queryByLabelText('打开网址')).toBeNull();
    await waitFor(() => expect(kinds()).toContain('show'));
    await user.click(screen.getByRole('button', { name: '主页' }));
    expect(command).toHaveBeenLastCalledWith({
      kind: 'navigate',
      sessionId: S,
      pageId: 'tab-home',
      input: 'about:blank',
    });
    area.mockRestore();
  });

  it('says why a load was refused and can put the notice away', async () => {
    const user = userEvent.setup();
    render(<BrowserPage sessionId={S} pageId="tab-1" visible />);
    await screen.findByLabelText('地址');
    act(() =>
      browserStore.put(
        page({ notice: { kind: 'blocked', text: 'blocked.example 已在浏览器设置中被阻止' } }),
      ),
    );
    expect(screen.getByRole('alert').textContent).toContain(
      'blocked.example 已在浏览器设置中被阻止',
    );
    await user.click(screen.getByRole('button', { name: '关闭提示' }));
    expect(command).toHaveBeenLastCalledWith({
      kind: 'dismiss-notice',
      sessionId: S,
      pageId: 'tab-1',
    });
  });

  it('offers to reopen a page that was released, and stays empty while the browser is off', async () => {
    const user = userEvent.setup();
    render(<BrowserPage sessionId={S} pageId="tab-1" visible />);
    await screen.findByLabelText('地址');
    act(() =>
      browserStore.put(
        page({
          closed: true,
          notice: { kind: 'closed', text: '为节省内存已关闭这个页面，再次打开即可' },
        }),
      ),
    );
    expect(screen.getByRole('status').textContent).toContain('为节省内存已关闭这个页面');
    const before = command.mock.calls.length;
    await user.click(screen.getByRole('button', { name: '重新打开' }));
    expect(command.mock.calls.slice(before).map(([value]) => value)).toContainEqual({
      kind: 'open',
      sessionId: S,
      pageId: 'tab-1',
    });

    command.mockResolvedValue({ ok: false, message: '内置浏览器已在设置中关闭' });
    render(<BrowserPage sessionId={S} pageId="tab-2" visible />);
    expect(await screen.findByText('内置浏览器已在设置中关闭')).toBeTruthy();
  });
});
