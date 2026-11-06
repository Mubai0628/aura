// @vitest-environment node
import { expect, test, vi } from 'vitest';
import type { OpenPanel } from './panels.js';
import { recheckPanels, refusalOf } from './recheck.js';
import type { PanelSite } from './site.js';

const S1 = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const S2 = 'ses_01BX5ZZKBKACTAV9WEVGEMMVRZ';

const site = (plugin = 'usage-panel', capabilities = ['session.read']): PanelSite => ({
  plugin,
  title: '本会话用量',
  folder: `C:\\plugins\\${plugin}`,
  entry: 'panel/index.html',
  network: [],
  capabilities,
});
/** Core's answer describes the panel without naming the plugin. */
const answer = (panelSite: PanelSite) => {
  const rest: Partial<PanelSite> = { ...panelSite };
  delete rest.plugin;
  return rest;
};

const panel = (sessionId: string, panelSite = site()) =>
  ({ sessionId, site: panelSite }) as unknown as OpenPanel;

const refusal = (message: string) => Object.assign(new Error('core'), { envelope: { message } });

test('an open panel Core no longer shows stops, with what Core said', async () => {
  const open = [panel(S1), panel(S2), panel(S1, site('qq-bridge'))];
  const stop = vi.fn();
  const ask = vi.fn(async (_method: string, params: Record<string, unknown>) => {
    if (params.session_id === S2)
      throw refusal('plugin_session.session_missing: 会话 ses_01BX… 不在了');
    if (params.plugin === 'qq-bridge')
      throw refusal('plugin_session.not_ready: 插件 qq-bridge 没有开启');
    return answer(site());
  });
  await recheckPanels({ list: () => open, stop }, ask);
  expect(ask).toHaveBeenCalledWith('agent.plugin_session.panel', {
    plugin: 'usage-panel',
    session_id: S1,
  });
  expect(stop.mock.calls).toEqual([
    [open[1], '会话 ses_01BX… 不在了'],
    [open[2], '插件 qq-bridge 没有开启'],
  ]);
});

test('a panel Core now describes differently stops, to open again as it is now', async () => {
  const open = [panel(S1)];
  const stop = vi.fn();
  await recheckPanels({ list: () => open, stop }, async () =>
    answer(site('usage-panel', ['session.read', 'session.send'])),
  );
  expect(stop).toHaveBeenCalledWith(open[0], '插件的面板改过了，重新打开就是改后的');
});

test('Core busy or restarting, or an answer Main cannot read, stops nothing', async () => {
  const stop = vi.fn();
  for (const failure of [
    new Error('plugin_panel.core_unavailable: Aura Core 还没有就绪'),
    refusal('ipc.timeout: Core 没有及时回答'),
    refusal('plugin_session.not_readyish: 不是这个代码'),
  ])
    await recheckPanels({ list: () => [panel(S1)], stop }, async () => {
      throw failure;
    });
  await recheckPanels({ list: () => [panel(S1)], stop }, async () => ({ odd: true }));
  expect(stop).not.toHaveBeenCalled();
  expect(refusalOf(refusal('plugin_session.panel_missing: x'))).toBe(
    'plugin_session.panel_missing: x',
  );
  expect(refusalOf('text')).toBe('');
});
