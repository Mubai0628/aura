import { expect, test } from 'vitest';

import {
  PROBE_NOT_SENT,
  probeFailure,
  projectionNotRead,
  SETTINGS_FILE_REJECTED,
  settingsNotRead,
  settingsNotSaved,
} from './failure-words';
import { UNCONFIRMED_SAVE } from './useAgentConfiguration';

// A tool-settings read or save that did not happen says why, what is in force, and what to do;
// a save whose outcome is not known says so as it is.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

test('a settings read that failed says why and what cannot be done until it is read', () => {
  expect(
    settingsNotRead(TIMEOUT, '代理设置', '现在不能修改代理；请稍后点“重新读取网络代理”。'),
  ).toBe(
    '代理设置没有读到（Aura Core 没有及时回复），现在不能修改代理；请稍后点“重新读取网络代理”。',
  );
  expect(
    settingsNotRead({ message: 'tools.configuration_unavailable' }, '工具配置', '其余。'),
  ).toBe('工具配置没有读到（Aura 内部出错），其余。');
  expect(
    settingsNotRead(
      {
        code: 'ipc.core_not_ready',
        message: 'Aura Core 尚未就绪，这次操作没有执行；请等它就绪后重试。',
      },
      '执行策略',
      '其余。',
    ),
  ).toBe('执行策略没有读到（Aura Core 还没有就绪），其余。');
  expect(
    settingsNotRead(new Error('boom'), '执行策略', '现在不能修改；请稍后点“重新读取执行策略”。'),
  ).toBe('执行策略没有读到，现在不能修改；请稍后点“重新读取执行策略”。');
});

test('a settings save that failed says why, what is in force, and what to do', () => {
  expect(
    settingsNotSaved(
      'tools.configuration_revision_conflict',
      '工具选择没有保存',
      '仍按原来的选择运行',
      '请稍后再保存',
    ),
  ).toBe(
    '工具选择没有保存（设置刚在别处被改过），仍按原来的选择运行。请重新打开这一页，核对后再改。',
  );
  expect(
    settingsNotSaved(
      'tools.invalid_setting:max_retries',
      '执行策略没有保存',
      '仍按原策略执行',
      '请检查后再保存',
      '最大重试次数',
    ),
  ).toBe('执行策略没有保存（「最大重试次数」超出允许范围），仍按原策略执行。请检查后再保存。');
  expect(
    settingsNotSaved(UNCONFIRMED_SAVE, '代理设置没有保存', '仍按原来的设置联网', '请检查后再保存'),
  ).toBe(UNCONFIRMED_SAVE);
  expect(
    settingsNotSaved(
      '代理端口已被占用。',
      '代理设置没有保存',
      '仍按原来的设置联网',
      '请检查后再保存',
    ),
  ).toBe('代理设置没有保存：代理端口已被占用。');
  expect(settingsNotSaved('boom', '网页服务没有切换', '仍在使用原来的服务', '请稍后再试')).toBe(
    '网页服务没有切换，仍在使用原来的服务。请稍后再试。',
  );
});

test('a proxy test that failed or was not sent says the proxy setting is unchanged', () => {
  expect(probeFailure(TIMEOUT)).toBe(
    '代理测试没有完成（Aura Core 没有及时回复），代理设置没有改变。请核对地址和端口后再测。',
  );
  expect(PROBE_NOT_SENT).toBe(
    '代理测试没有发出去（Aura 内部通信失败），代理设置没有改变；请稍后再测。',
  );
});

test('a snapshot that is not read yet says why and that it is asked for again', () => {
  expect(projectionNotRead('工具设置', 'ipc.core_not_ready')).toBe(
    '工具设置还没有读到（Aura Core 还没有就绪），正在重试；一直这样请重启 Aura。',
  );
  expect(projectionNotRead('HTTP 工具凭据绑定', 'bridge_rejected')).toBe(
    'HTTP 工具凭据绑定还没有读到（Aura 内部通信失败），正在重试；一直这样请重启 Aura。',
  );
  expect(projectionNotRead('工具设置', 'tool_projection.unknown')).toBe(
    '工具设置还没有读到，正在重试；一直这样请重启 Aura。',
  );
});

test('tool settings rejected from outside say what is in force and how to replace them', () => {
  expect(SETTINGS_FILE_REJECTED).toEqual({
    title: '工具设置文件里的改动无效',
    body: 'Aura 正在使用上次有效的设置；在本页改好后保存即可覆盖它。',
  });
});
