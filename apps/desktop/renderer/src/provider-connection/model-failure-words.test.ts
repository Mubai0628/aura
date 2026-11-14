import { expect, test } from 'vitest';

import {
  catalogNotRefreshed,
  catalogNotRefreshedBy,
  catalogPageFailure,
  catalogUnavailableText,
  CATALOG_ENTRY_MISSING,
  CATALOG_ENTRY_UPDATED,
  clearCredentialFailure,
  COMPATIBILITY_TEXT,
  CREDENTIAL_CLEARED,
  credentialStateRefusal,
  modelFailureText,
  routeSaveFailure,
  saveCredentialFailure,
  settingsChanging,
  settingsNotReadLatest,
} from './model-failure-words';
import { availabilityPrerequisite, credentialStatusTitle } from './credential-status';

// A model setting that was not written says why and what to do; one Core did not answer says it
// may have taken effect; a key that was not saved or cleared says which, and what the connection
// uses now.

const TIMEOUT = { code: 'ipc.request_timeout', message: 'Core request timed out' };

test('a model write Core did not answer may have taken effect; a read says it was not read', () => {
  expect(modelFailureText(TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复，这次更改可能已经生效。请先刷新模型设置核对，再决定是否重做。',
  );
  expect(modelFailureText(TIMEOUT, true)).toBe(
    '没有等到 Aura Core 的答复，这次没有读到；请稍后再试。',
  );
  expect(modelFailureText({ code: 'x', message: 'y' })).toBe(
    '操作没有完成。请刷新模型设置，核对当前状态后重试。',
  );
  expect(modelFailureText({ code: 'broker.facts_changed', message: 'broker.facts_changed' })).toBe(
    '模型设置正在被另一项操作修改，这次更改没有执行，请稍后再试。',
  );
});

test('a key that was not saved says why and that the box was emptied', () => {
  expect(saveCredentialFailure(TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复，无法确认 API Key 是否已保存。请刷新模型设置查看，仍是未配置时重新输入保存。',
  );
  expect(
    saveCredentialFailure({ code: 'credential.authentication_scheme_mismatch', message: 'x' }),
  ).toBe(
    '这个连接的认证方式不使用 API Key，API Key 没有保存。要用 API Key，请先把认证方式改成 API Key 并保存连接。',
  );
  expect(saveCredentialFailure({ code: 'x', message: 'y' })).toBe(
    'API Key 未保存，配置没有改变。输入框已清空，请重新输入 API Key 再保存。',
  );
  expect(
    saveCredentialFailure({ code: 'x', message: 'credential.other: 凭据存储暂时不可用。' }),
  ).toBe('凭据存储暂时不可用。输入框已清空，请重新输入 API Key 再保存。');
});

test('a key slot that cannot take a new key says why and to clear it first', () => {
  expect(credentialStateRefusal('unavailable')).toBe(
    'Windows 凭据管理器里读不到这个连接原来的 API Key，不能直接替换，设置没有改变。请先点“清除”，再重新输入并保存。',
  );
  expect(credentialStateRefusal('reconciliation_required')).toBe(
    '这个连接上次写入 API Key 没有确认完成，暂时不能保存新的，设置没有改变。请先点“清除”，再重新输入并保存。',
  );
  expect(credentialStatusTitle({ state: 'unavailable' } as never)).toBe(
    'API Key：Windows 凭据管理器里读不到，请清除后重新保存',
  );
  expect(credentialStatusTitle({ state: 'reconciliation_required' } as never)).toBe(
    'API Key：上次写入没有确认完成，暂时不能保存新的；可先清除再保存',
  );
});

test('a connection that cannot be tested yet names the key it lacks', () => {
  expect(availabilityPrerequisite('open_ai_bearer_api_key', undefined)).toBe(
    '这个连接还没有 API Key，保存 API Key 后才能测试。',
  );
  expect(availabilityPrerequisite('open_ai_bearer_api_key', 'cleared')).toBe(
    '这个连接还没有 API Key，保存 API Key 后才能测试。',
  );
  expect(availabilityPrerequisite('open_ai_bearer_api_key', 'unavailable')).toBe(
    'Windows 凭据管理器里读不到这个连接的 API Key，清除后重新保存才能测试。',
  );
  expect(availabilityPrerequisite('open_ai_bearer_api_key', 'reconciliation_required')).toBe(
    '这个连接上次写入 API Key 没有确认完成，清除后重新保存才能测试。',
  );
  expect(availabilityPrerequisite('open_ai_bearer_api_key', 'configured')).toBeUndefined();
  expect(availabilityPrerequisite('none', undefined)).toBeUndefined();
});

test('a key that was not cleared says it is still used; a cleared key says it is no longer sent', () => {
  expect(clearCredentialFailure({ code: 'credential.store_unavailable', message: 'x' })).toBe(
    'Windows 凭据管理器拒绝了删除，API Key 仍然保存着，这个连接照常使用它。请稍后再点清除。',
  );
  expect(clearCredentialFailure(TIMEOUT)).toBe(
    '没有等到 Aura Core 的答复，无法确认 API Key 是否已清除。请刷新模型设置查看。',
  );
  expect(clearCredentialFailure({ code: 'x', message: 'y' })).toBe(
    'API Key 没有清除，这个连接照常使用它；请稍后再点清除。',
  );
  expect(CREDENTIAL_CLEARED).toBe('API Key 已清除，这个连接不再发送密钥。');
});

test('connections saved one after another say which were saved and which were not tried', () => {
  expect(
    routeSaveFailure(['A', 'B'], 'C', ['D'], {
      code: 'x',
      message: 'models.endpoint.conflict: 这个地址已被另一个连接使用。',
    }),
  ).toBe(
    '已保存 A、B；C 没有保存（这个地址已被另一个连接使用），之后的 D 没有尝试。请检查 C 后再保存剩下的连接。',
  );
  expect(routeSaveFailure([], 'C', [], TIMEOUT)).toBe(
    'C 是否已保存没有确认（Aura Core 没有及时回复）。请先刷新模型设置核对，再决定是否重新保存。',
  );
  expect(routeSaveFailure(['A'], 'C', [], { code: 'x', message: 'y' })).toBe(
    '已保存 A；C 没有保存。请检查 C 后再保存一次。',
  );
});

test('the compatibility record is named in words', () => {
  expect(COMPATIBILITY_TEXT).toEqual({
    heading: '兼容记录',
    reset: '重置',
    none: '这个连接还没有记下兼容调整。',
    unread: '兼容记录没有读到，现在看不出有没有记录；请关闭后重新打开这个连接。',
    resetFailed: '兼容记录未重置',
    resetDone: '兼容记录已重置',
    cleared: '这个连接记下的兼容调整已清除。',
    nothing: '这个连接没有记下兼容调整，无需重置。',
  });
});

test('settings being written elsewhere say whether they are read again and if saving waits', () => {
  expect(settingsChanging(true, true)).toBe(
    '模型设置正在被另一项操作修改，为免覆盖暂不能保存；稍后会自动再读。',
  );
  expect(settingsChanging(false, true)).toBe(
    '模型设置正在被另一项操作修改，这次没有读到；稍后会自动再读。',
  );
  expect(settingsChanging(true, false)).toBe(
    '模型设置正在被另一项操作修改，为免覆盖暂不能保存；请稍后重新打开设置。',
  );
});

test('settings that were not read again say why, and that saving waits', () => {
  expect(
    settingsNotReadLatest({ code: 'ipc.transport_lost', message: 'Transport is closed' }),
  ).toBe(
    '模型设置没有读到最新状态（与 Aura Core 的连接断开了），现有配置仍可查看；为免覆盖暂不能保存，连接恢复后会自动再读。',
  );
  expect(
    settingsNotReadLatest({ code: 'x', message: 'models.store: 模型设置存储暂时不可用。' }),
  ).toBe(
    '模型设置没有读到最新状态（模型设置存储暂时不可用），现有配置仍可查看；为免覆盖暂不能保存，请重新打开设置再读。',
  );
  expect(settingsNotReadLatest(new Error('boom'))).toBe(
    '模型设置没有读到最新状态，现有配置仍可查看；为免覆盖暂不能保存，请重新打开设置再读。',
  );
});

test('a catalog that was not refreshed or is unavailable says why and what is shown', () => {
  expect(catalogNotRefreshed('provider.catalog_refresh_failed')).toBe(
    'models.dev 目录没有刷新（连不上 models.dev），模型列表仍是上次的。请检查网络或代理后再刷新。',
  );
  expect(catalogNotRefreshed('provider.catalog_invalid')).toBe(
    'models.dev 目录没有刷新（models.dev 返回的目录无法识别），模型列表仍是上次的。请稍后再刷新。',
  );
  expect(catalogNotRefreshedBy(TIMEOUT)).toBe(
    'models.dev 目录没有刷新（Aura Core 没有及时回复）；请稍后再刷新。',
  );
  expect(catalogUnavailableText('persistence.catalog_cache_failed')).toBe(
    'models.dev 目录暂不可用（本机的目录缓存读写失败），服务商列表无法显示。请稍后刷新目录。',
  );
  expect(catalogUnavailableText(null)).toBe(
    'models.dev 目录暂不可用，服务商列表无法显示。请检查网络或代理后刷新目录。',
  );
  expect(catalogUnavailableText(undefined, TIMEOUT)).toBe(
    'models.dev 目录暂不可用（Aura Core 没有及时回复），服务商列表无法显示。请稍后刷新目录。',
  );
});

test('catalog entries of one model say what changed, or why nothing did', () => {
  expect(CATALOG_ENTRY_MISSING).toBe(
    'models.dev 目录里找不到这个模型 ID，目录信息没有更新，设置不变。请确认模型 ID 是否写对。',
  );
  expect(CATALOG_ENTRY_UPDATED).toBe('已按目录更新这个模型的信息；你改过的设置和备注不变。');
  expect(catalogPageFailure(TIMEOUT)).toBe(
    '目录模型没有读到（Aura Core 没有及时回复），请稍后点“搜索”重试。',
  );
  expect(catalogPageFailure({ code: 'x', message: 'y' })).toBe(
    '目录模型没有读到，请稍后点“搜索”重试。',
  );
});
