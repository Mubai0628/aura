import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ToolPackagesPanel } from './ToolPackagesPanel';
import raw from '../../../../../fixtures/contracts/package-routes.v1.json?raw';

// The Rust-generated route values, so this page is tested against what Core really sends.
// eslint-disable-next-line @typescript-eslint/no-explicit-any -- a fixture document of many shapes
const fixture = JSON.parse(raw) as Record<string, any>;
afterEach(cleanup);

function setup(answers: Record<string, unknown[]>) {
  const invoke = vi.fn(async (command: string) => {
    const queue = answers[command];
    if (!queue || queue.length === 0) throw new Error(`unexpected ${command}`);
    return { ok: true, value: queue.length > 1 ? queue.shift() : queue[0] };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { packages: { invoke }, listToolCredentials: vi.fn() } },
  });
  render(<ToolPackagesPanel />);
  return invoke;
}

test('import shows what the package asks for; verification and install each need their own approval', async () => {
  const view = fixture.version_view;
  const notRun = {
    ...view,
    conformance: {
      ...view.conformance,
      state: 'not_run',
      report: null,
      report_hash: null,
      run_id: null,
      started_at: null,
    },
  };
  const invoke = setup({
    'packages.list': [{ packages: [] }, { packages: [] }, fixture.list],
    'packages.import_directory': [{ type: 'imported', view: notRun }],
    'packages.conformance.prepare': [fixture.preparation],
    'packages.conformance.start': [fixture.conformance],
    'packages.install': [fixture.list.packages[0]],
  });
  expect(await screen.findByText('尚未导入工具包。')).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '导入目录' }));
  expect(await screen.findByText(/这个包的代码会以你的 Windows 用户身份运行/)).toBeTruthy();
  expect(invoke).toHaveBeenCalledWith('packages.import_directory', {});
  expect(screen.queryByRole('button', { name: /安装/ })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: '准备验证' }));
  expect(await screen.findByRole('dialog', { name: '批准运行验证' })).toBeTruthy();
  expect(invoke).not.toHaveBeenCalledWith('packages.conformance.start', expect.anything());
  fireEvent.click(screen.getByRole('button', { name: '批准并运行验证' }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith('packages.conformance.start', {
      preparation_id: fixture.preparation.preparation_id,
      preview_hash: fixture.preparation.preview_hash,
    }),
  );
  fireEvent.click(await screen.findByRole('button', { name: '安装：信任这个版本并启用' }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith('packages.install', {
      version: fixture.conformance.version,
      expected_package_revision: notRun.package_revision,
      report_hash: fixture.conformance.report_hash,
    }),
  );
  expect((await screen.findAllByText(/已安装/)).length).toBeGreaterThan(0);
});

test('import diagnostics are shown and a dismissed dialog changes nothing', async () => {
  const invoke = setup({
    'packages.list': [{ packages: [] }],
    'packages.import_archive': [
      {
        type: 'rejected',
        diagnostics: [
          {
            code: 'invalid_manifest',
            location: { type: 'manifest', line: 3, column: 1, field: 'tools' },
            message: '清单字段不合法。',
          },
        ],
      },
      { type: 'cancelled' },
    ],
  });
  fireEvent.click(await screen.findByRole('button', { name: '导入 ZIP' }));
  expect(await screen.findByText(/清单字段不合法。（清单第 3 行，tools）/)).toBeTruthy();
  fireEvent.click(screen.getByRole('button', { name: '导入 ZIP' }));
  await waitFor(() => expect(invoke).toHaveBeenCalledTimes(5));
  expect(screen.queryByRole('dialog')).toBeNull();
});

test('lifecycle changes show their impact and are applied only after confirmation', async () => {
  const summary = fixture.list.packages[0];
  const invoke = setup({
    'packages.list': [fixture.list],
    'packages.set_enabled': [summary],
  });
  fireEvent.click(await screen.findByRole('button', { name: '管理' }));
  fireEvent.click(screen.getByRole('button', { name: '停用' }));
  expect(screen.getByRole('dialog', { name: '确认工具包更改' }).textContent).toMatch(
    /正在运行的调用允许跑完/,
  );
  expect(invoke).not.toHaveBeenCalledWith('packages.set_enabled', expect.anything());
  fireEvent.click(screen.getByRole('button', { name: '确认' }));
  await waitFor(() =>
    expect(invoke).toHaveBeenCalledWith('packages.set_enabled', {
      package_id: summary.package_id,
      expected_package_revision: summary.revision,
      enabled: false,
    }),
  );
});
