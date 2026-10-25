import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, test, vi } from 'vitest';
import { App } from './App';
import { installAppTestEnvironment, workspaceSessionProjection } from './app-test-fixtures';

installAppTestEnvironment();

describe('P05 prerequisite desktop shell', () => {
  test('renders real empty history, quick selectors and keyboard resizers without demo content', async () => {
    const user = userEvent.setup();
    const { container } = render(<App />);

    expect(container.querySelector('.app-topbar')).toBeNull();
    expect(await screen.findByRole('heading', { level: 2, name: '合成桌面会话' })).toBeTruthy();
    await waitFor(() => expect(screen.getByText('Aura Core 已就绪')).toBeTruthy());
    expect(screen.getByText('本地运行 · 无需登录')).toBeTruthy();
    expect(screen.queryByRole('button', { name: '登录' })).toBeNull();
    expect(await screen.findByRole('heading', { name: '开始一个新任务' })).toBeTruthy();
    expect(screen.queryByText(/不是 Core Projection/u)).toBeNull();
    expect(
      screen.getByRole('progressbar', { name: '上下文使用情况' }).getAttribute('aria-valuenow'),
    ).toBe('0');
    expect(screen.getByRole('complementary', { name: '右侧工作区' })).toBeTruthy();
    expect(container.querySelector('.conversation-header p')).toBeNull();
    const conversationCanvas = container.querySelector('.conversation-canvas');
    const conversationScroll = container.querySelector('.conversation-scroll');
    const conversationDock = container.querySelector('.conversation-dock');
    expect(conversationCanvas).toBeTruthy();
    expect(conversationScroll?.parentElement).toBe(conversationCanvas);
    expect(conversationDock?.parentElement).toBe(conversationCanvas);

    await user.click(screen.getByRole('button', { name: /上下文占用/u }));
    // The card's note says it; the short card has no second line for the same words.
    expect(screen.getByRole('region', { name: '上下文占用详情' }).textContent).toContain(
      '本会话还没有形成模型请求',
    );
    await user.keyboard('{Escape}');

    await user.click(screen.getByRole('button', { name: /工作区读写/u }));
    const permissionMenu = screen.getByRole('menu', { name: '快速选择权限' });
    await user.click(within(permissionMenu).getByRole('menuitem', { name: '会话授权管理' }));
    // B04: the production entry opens exactly one modal layer, and closing it
    // takes that layer out of the top layer.
    expect(screen.getAllByRole('heading', { name: '会话授权管理' })).toHaveLength(1);
    expect(document.querySelectorAll('dialog.modal-backdrop[open]')).toHaveLength(1);
    await user.click(screen.getByRole('button', { name: '关闭授权管理' }));
    expect(screen.queryByRole('heading', { name: '会话授权管理' })).toBeNull();
    expect(document.querySelectorAll('dialog.modal-backdrop[open]')).toHaveLength(0);
    await user.click(screen.getByRole('button', { name: /工作区读写/u }));
    await user.click(
      within(screen.getByRole('menu', { name: '快速选择权限' })).getByRole('menuitemradio', {
        name: '只读',
      }),
    );
    expect(screen.getByRole('button', { name: /只读/u })).toBeTruthy();
    await user.click(screen.getByRole('button', { name: /只读/u }));
    await user.click(
      within(screen.getByRole('menu', { name: '快速选择权限' })).getByRole('menuitemradio', {
        name: '只读',
      }),
    );
    expect(screen.getByRole('button', { name: /只读/u })).toBeTruthy();

    await user.click(screen.getByRole('button', { name: /正在加载模型/u }));
    const modelMenu = screen.getByRole('menu', { name: '快速选择模型' });
    expect(within(modelMenu).getByText('正在读取模型配置…')).toBeTruthy();
    await user.click(screen.getByRole('heading', { name: '合成桌面会话' }));
    expect(screen.queryByRole('menu', { name: '快速选择模型' })).toBeNull();

    await user.click(screen.getByRole('button', { name: '关闭右侧边栏' }));
    expect(screen.queryByRole('complementary', { name: '右侧工作区' })).toBeNull();
    await user.click(screen.getByRole('button', { name: '展开右侧边栏' }));
    expect(screen.getByRole('complementary', { name: '右侧工作区' })).toBeTruthy();

    const leftSeparator = screen.getByRole('separator', { name: '调整左侧工作区宽度' });
    leftSeparator.focus();
    await user.keyboard('{Enter}');
    expect(screen.queryByRole('complementary', { name: '工作区导航' })).toBeNull();
    await user.click(screen.getByRole('button', { name: '展开左侧边栏' }));
    expect(screen.getByRole('complementary', { name: '工作区导航' })).toBeTruthy();

    const rightSeparator = screen.getByRole('separator', { name: '调整右侧工作区宽度' });
    const initialRightWidth = Number(rightSeparator.getAttribute('aria-valuenow'));
    const maximumRightWidth = Number(rightSeparator.getAttribute('aria-valuemax'));
    rightSeparator.focus();
    await user.keyboard('{ArrowLeft}');
    expect(Number(rightSeparator.getAttribute('aria-valuenow'))).toBe(
      Math.min(maximumRightWidth, initialRightWidth + 16),
    );

    const composerSeparator = screen.getByRole('separator', { name: '调整消息编辑器高度' });
    const initialComposerHeight = Number(composerSeparator.getAttribute('aria-valuenow'));
    composerSeparator.focus();
    await user.keyboard('{ArrowUp}');
    expect(Number(composerSeparator.getAttribute('aria-valuenow'))).toBe(
      initialComposerHeight + 12,
    );
    expect(screen.queryByRole('heading', { name: 'Todo' })).toBeNull();

    await user.click(screen.getByRole('button', { name: /^文件/u }));
    expect(screen.queryByText('目录预览 · 演示数据')).toBeNull();
  });

  /**
   * The v5 acceptance recorded 135 React duplicate-key warnings whose key was a
   * session id, changing with every session it created. The keys were not in the
   * sidebar list — that one renders each session once — but in the conversation
   * dock, where two different siblings both carried the bare session id. React
   * cannot tell those apart, so it reuses the first child's state for the second
   * across updates; the warning count is the only visible trace of that. The tray
   * only *renders* when an approval is pending, but its element is in the list on
   * every render, which is why the warning repeated for as long as the window was
   * open rather than only while something was being approved.
   *
   * One quiet render of the real dock is the whole reproduction, and every render
   * moment has to stay quiet: the first paint, a new local conversation, and a
   * switch back to the session the projection reports.
   */
  test('renders every keyed dock sibling under its own identity', async () => {
    const user = userEvent.setup();
    const errors: string[] = [];
    const consoleError = vi.spyOn(console, 'error').mockImplementation((...args: unknown[]) => {
      errors.push(args.map(String).join(' '));
    });
    const duplicateKeys = () =>
      errors.filter((message) => message.includes('Encountered two children with the same key'));
    const sessionRows = (container: HTMLElement) => [
      ...container.querySelectorAll<HTMLElement>('.session-nav-item'),
    ];
    try {
      const { container } = render(<App />);
      await screen.findByRole('heading', { name: '合成桌面会话' });
      expect(container.querySelector('.conversation-dock-inner')).toBeTruthy();
      expect(container.querySelector('.composer')).toBeTruthy();
      expect(duplicateKeys()).toEqual([]);

      await user.click(screen.getByRole('button', { name: '在 合成工作区 新建会话' }));
      await screen.findByRole('heading', { name: '新会话' });
      expect(duplicateKeys()).toEqual([]);
      expect(sessionRows(container)).toHaveLength(2);

      await user.click(screen.getByRole('button', { name: '合成桌面会话' }));
      await screen.findByRole('heading', { name: '合成桌面会话' });
      expect(duplicateKeys()).toEqual([]);
      expect(container.querySelector('.composer')).toBeTruthy();

      const rows = sessionRows(container);
      const identities = rows.map((row) => row.textContent ?? '');
      expect(new Set(identities).size, `duplicated rows: ${identities.join(' | ')}`).toBe(
        identities.length,
      );
      const selected = rows.filter((row) => row.querySelector('[aria-current="page"]') !== null);
      expect(selected).toHaveLength(1);
      expect(selected[0]?.textContent).toContain('合成桌面会话');
    } finally {
      consoleError.mockRestore();
    }
  });

  test('dismisses every click-triggered expansion card when clicking outside', async () => {
    const user = userEvent.setup();
    render(<App />);
    const conversationHeading = await screen.findByRole('heading', {
      name: '合成桌面会话',
    });
    // The + menu adds context; the task modes are /plan and /goal.
    await user.click(screen.getByRole('button', { name: '添加' }));
    expect(screen.getByRole('menu', { name: '添加' }).textContent).toContain('Skill');
    expect(screen.getByRole('menu', { name: '添加' }).textContent).not.toContain('制定计划');
    await user.click(conversationHeading);
    expect(screen.queryByRole('menu', { name: '添加' })).toBeNull();

    await user.click(screen.getByRole('button', { name: /工作区读写/u }));
    expect(screen.getByRole('menu', { name: '快速选择权限' })).toBeTruthy();
    await user.click(conversationHeading);
    expect(screen.queryByRole('menu', { name: '快速选择权限' })).toBeNull();

    await user.click(screen.getByRole('button', { name: /正在加载模型/u }));
    expect(screen.getByRole('menu', { name: '快速选择模型' })).toBeTruthy();
    await user.click(conversationHeading);
    expect(screen.queryByRole('menu', { name: '快速选择模型' })).toBeNull();

    await user.click(screen.getByRole('button', { name: '打开工具' }));
    expect(screen.getByRole('menu', { name: '选择工具' })).toBeTruthy();
    await user.click(conversationHeading);
    expect(screen.queryByRole('menu', { name: '选择工具' })).toBeNull();
  });

  test('an archived Workspace leaves the main list while its data stays registered', async () => {
    // 设置 → 归档 owns archived Workspaces; the main list only offers archive, so
    // a retired Workspace must not keep a sidebar row (its Sessions go with it).
    vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
      ok: true,
      value: {
        ...workspaceSessionProjection,
        payload: {
          sessions: [
            ...workspaceSessionProjection.payload.sessions,
            {
              session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              title: '已归档工作区的会话',
              state: 'idle',
            },
          ],
          workspaces: [
            ...workspaceSessionProjection.payload.workspaces,
            {
              workspace_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              display_name: '已归档工作区',
              selected_root: 'C:/synthetic-archived',
              canonical_root: 'C:/synthetic-archived',
              authorization_epoch: '1',
              revision: '3',
              catalog_freshness: 'unavailable',
              state: 'archived',
            },
          ],
        },
      },
    });
    render(<App />);

    expect(await screen.findByRole('button', { name: '合成工作区' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '已归档工作区' })).toBeNull();
    expect(screen.queryByText('已归档工作区的会话')).toBeNull();
    // Mocks in this file persist across cases; leave the default projection behind.
    vi.mocked(window.aura.v1.getWorkspaceSessions).mockResolvedValue({
      ok: true,
      value: workspaceSessionProjection,
    });
  });

  test('keeps the P04 security control surface under Settings and preserves Core authority', async () => {
    const user = userEvent.setup();
    render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(screen.getByRole('button', { name: '权限与沙箱' }));

    expect(screen.getByRole('heading', { name: '权限与沙箱' })).toBeTruthy();
    expect(screen.getByRole('heading', { name: '隔离与下载' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '重新探测' })).toBeNull();
    // No explanatory line under the panel or its heading.
    expect(screen.queryByText(/兼容环境用于运行隔离工具|可执行下载保持隔离/u)).toBeNull();
    expect(screen.getByRole('button', { name: '配置兼容环境' })).toBeTruthy();
    expect(screen.queryByRole('button', { name: '继续并请求管理员权限' })).toBeNull();
  });

  test('routes a fail-closed Release Trust error through the dismissable top notification', async () => {
    const user = userEvent.setup();
    vi.mocked(window.aura.v1.prepareHostPreparation).mockResolvedValue({
      error: {
        category: 'unavailable',
        code: 'sandbox.host_preparation_release_trust_failed',
        correlation_id: 'cor_01ARZ3NDEKTSV4RRFFQ69G5FAV',
        details: {},
        message: 'fixture Release Trust failure',
        retryable: false,
        terminal: 'failed',
      },
      ok: false,
    });
    const { container } = render(<App />);

    await user.click(screen.getByRole('button', { name: '设置' }));
    await user.click(screen.getByRole('button', { name: '权限与沙箱' }));
    const hostReadsBefore = vi.mocked(window.aura.v1.getHostPreparationStatus).mock.calls.length;
    await user.click(await screen.findByRole('button', { name: '配置兼容环境' }));
    await waitFor(() =>
      expect(vi.mocked(window.aura.v1.getHostPreparationStatus).mock.calls.length).toBeGreaterThan(
        hostReadsBefore,
      ),
    );
    expect(window.aura.v1.prepareHostPreparation).toHaveBeenCalledTimes(1);

    const alert = screen.getByRole('alert');
    expect(alert.textContent).toContain('当前 Release Bundle 未通过 Aura Production Root 信任验证');
    expect(alert.textContent).toContain('fail-closed');
    expect(container.querySelector('.security-error')).toBeNull();
    await user.click(within(alert).getByRole('button', { name: '关闭消息' }));
    expect(screen.queryByRole('alert')).toBeNull();
  });
});
