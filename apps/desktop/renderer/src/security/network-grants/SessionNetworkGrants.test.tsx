import { cleanup, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, expect, test, vi } from 'vitest';
import { SessionNetworkGrants } from './SessionNetworkGrants';
afterEach(cleanup);

test('Git consent names its exact branch and repository rather than all destinations', async () => {
  const list = vi.fn().mockResolvedValue({
    ok: true,
    value: [
      {
        grant_id: 'git-grant',
        grant_revision: '1',
        availability: 'in_force',
        expires_at: null,
        scope: {
          caller: 'git',
          destinations: {
            git: {
              target: {
                remote_url: 'https://fixture.invalid/repo.git',
                remote_branch: 'review',
                repository: {
                  algorithm: 'sha256',
                  canonicalization_version: 'rfc8785_jcs_v1',
                  hex: 'a'.repeat(64),
                },
              },
            },
          },
        },
      },
    ],
  });
  Object.assign(window, { aura: { v1: { listNetworkSessionConsents: list } } });
  render(<SessionNetworkGrants sessionId="session-a" open />);
  expect(
    await screen.findByText(
      /Git 普通推送批准.*fixture.invalid\/repo.git.*review.*仓库 aaaaaaaaaaaa/u,
    ),
  ).toBeTruthy();
  expect(screen.queryByText(/Git 普通推送批准 · 全部目标/u)).toBeNull();
});

test('shows durable scope and revokes the exact revision without claiming an unconfirmed stop', async () => {
  const grant = {
    grant_id: 'network-grant',
    grant_revision: '3',
    availability: 'in_force',
    expires_at: null,
    scope: { caller: 'ordinary_tools' },
  };
  const list = vi.fn().mockResolvedValue({ ok: true, value: [grant] });
  const revoke = vi.fn().mockImplementation(async () => {
    list.mockResolvedValue({ ok: true, value: [{ ...grant, availability: 'revoked' }] });
    return { ok: false, error: { message: '批准已撤销；停止未确认，请核对运行状态。' } };
  });
  Object.assign(window, {
    aura: { v1: { listNetworkSessionConsents: list, revokeNetworkSessionConsent: revoke } },
  });
  render(<SessionNetworkGrants sessionId="session-a" open />);
  expect(await screen.findByText(/全部目标 · 生效中/u)).toBeTruthy();
  // The section lists the approvals; their rules were shown when they were given.
  expect(
    screen.queryByText(/重启和模式切换不会撤销|旧网络批准|只覆盖列出|独立于运行批准/u),
  ).toBeNull();
  await userEvent.click(screen.getByRole('button', { name: '撤销联网批准' }));
  expect(revoke).toHaveBeenCalledWith({
    session_id: 'session-a',
    grant_id: 'network-grant',
    expected_revision: '3',
  });
  expect(await screen.findByText(/停止未确认/u)).toBeTruthy();
  expect(screen.queryByText(/已确认停止/u)).toBeNull();
});

test('labels Shell network answers separately from ordinary HTTP and web answers', async () => {
  const list = vi.fn().mockResolvedValue({
    ok: true,
    value: [
      {
        grant_id: 'shell-network',
        grant_revision: '1',
        availability: 'in_force',
        expires_at: null,
        scope: { caller: 'built_in_shells' },
      },
      {
        grant_id: 'ordinary-network',
        grant_revision: '2',
        availability: 'in_force',
        expires_at: null,
        scope: { caller: 'ordinary_tools' },
      },
    ],
  });
  Object.assign(window, { aura: { v1: { listNetworkSessionConsents: list } } });
  render(<SessionNetworkGrants sessionId="session-a" open />);
  expect(await screen.findByText(/Shell 联网批准 · 全部目标/u)).toBeTruthy();
  expect(screen.getByText(/联网（普通网络工具） · 全部目标/u)).toBeTruthy();
  // That the proxy variables are not enforced is said in the Shell approval's details.
  expect(screen.queryByText(/代理环境变量.*未强制/u)).toBeNull();
});

test('does not display stale network grants after switching sessions', async () => {
  const list = vi.fn().mockResolvedValue({ ok: true, value: [] });
  Object.assign(window, { aura: { v1: { listNetworkSessionConsents: list } } });
  const view = render(<SessionNetworkGrants sessionId="session-a" open />);
  await screen.findByText('当前没有会话联网批准。');
  list.mockReturnValue(new Promise(() => undefined));
  view.rerender(<SessionNetworkGrants sessionId="session-b" open />);
  expect(screen.getByText('正在读取联网批准…')).toBeTruthy();
  expect(screen.queryByText('当前没有会话联网批准。')).toBeNull();
});

test('a package answer names its package version, not all destinations', async () => {
  const list = vi.fn().mockResolvedValue({
    ok: true,
    value: [
      {
        grant_id: 'package-grant',
        grant_revision: '1',
        availability: 'in_force',
        expires_at: null,
        scope: {
          caller: 'package_tools',
          destinations: {
            package: {
              package_id: 'tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              package_fingerprint: {
                algorithm: 'sha256',
                canonicalization_version: 'rfc8785_jcs_v1',
                hex: 'c'.repeat(64),
              },
            },
          },
        },
      },
    ],
  });
  const packages = vi.fn().mockResolvedValue({
    ok: true,
    value: {
      packages: [{ package_id: 'tpk_01ARZ3NDEKTSV4RRFFQ69G5FAV', display_name: 'Fixture Tools' }],
    },
  });
  Object.assign(window, {
    aura: { v1: { listNetworkSessionConsents: list, packages: { invoke: packages } } },
  });
  render(<SessionNetworkGrants sessionId="session-a" open />);
  expect(
    await screen.findByText(/工具包批准 · 工具包「Fixture Tools」（内容指纹 cccccccccccc）/u),
  ).toBeTruthy();
  expect(packages).toHaveBeenCalledWith('packages.list', {});
  expect(screen.queryByText(/工具包批准 · 全部目标/u)).toBeNull();
});
