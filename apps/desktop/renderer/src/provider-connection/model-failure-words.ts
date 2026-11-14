import { answerMissing, connectionCause, isConflict } from '../agent-session/failure-text';
import { userMessage } from '../agent-session/user-message';

/**
 * What a model setting that was not written or read says: why, whether it may have taken effect,
 * and what to do; a key that was not saved or cleared says which, and what the connection uses
 * now; the models.dev catalog says why it was not refreshed and what is shown instead.
 */

/** Core refused a request because another change to the same settings was still being written. */
export const FACTS_CHANGED = 'broker.facts_changed';

/** When Core's answer has no sentence the user can read. */
export const MODEL_FAILURE_FALLBACK = '操作没有完成。请刷新模型设置，核对当前状态后重试。';

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** Core's or Aura's own sentence for a failure, as a clause; none when there is none to read. */
const said = (error: unknown): string | undefined => {
  const text = userMessage(error, '');
  return text === '' ? undefined : text.replace(/[。.！!]+$/u, '');
};

/** A model write (or, with `read`, a read) that did not take effect or was not answered. */
export function modelFailureText(error: unknown, read = false): string {
  if (codeOf(error) === FACTS_CHANGED)
    return read
      ? '模型设置正在被另一项操作修改，这次没有读到；请稍后再试。'
      : '模型设置正在被另一项操作修改，这次更改没有执行，请稍后再试。';
  if (answerMissing(error))
    return read
      ? '没有等到 Aura Core 的答复，这次没有读到；请稍后再试。'
      : '没有等到 Aura Core 的答复，这次更改可能已经生效。请先刷新模型设置核对，再决定是否重做。';
  return userMessage(error, read ? '这次没有读到，请稍后再试。' : MODEL_FAILURE_FALLBACK);
}

/** Settings being written elsewhere: whether saving waits (`held`) and they are read again. */
export const settingsChanging = (held: boolean, rereading: boolean): string =>
  `模型设置正在被另一项操作修改，${held ? '为免覆盖暂不能保存' : '这次没有读到'}；${
    rereading ? '稍后会自动再读' : '请稍后重新打开设置'
  }。`;

/** Settings shown from an earlier read, as the latest could not be read; saving waits. */
export function settingsNotReadLatest(error: unknown): string {
  const cause = connectionCause(error);
  if (cause !== undefined)
    return `模型设置没有读到最新状态（${cause}），现有配置仍可查看；为免覆盖暂不能保存，连接恢复后会自动再读。`;
  const reason = said(error);
  return `模型设置没有读到最新状态${reason ? `（${reason}）` : ''}，现有配置仍可查看；为免覆盖暂不能保存，请重新打开设置再读。`;
}

const CREDENTIAL_SAVE_REFUSALS: Readonly<Record<string, string>> = {
  'credential.authentication_scheme_mismatch':
    '这个连接的认证方式不使用 API Key，API Key 没有保存。要用 API Key，请先把认证方式改成 API Key 并保存连接。',
  'credential.state_conflict':
    'API Key 的状态刚有变化，这次没有保存；请关闭后重新打开这个连接，再输入保存。',
  'credential.row_revision_conflict':
    'API Key 的状态刚有变化，这次没有保存；请关闭后重新打开这个连接，再输入保存。',
  'credential.mutation_aborted':
    'API Key 写入在完成前被中止，没有保存，配置没有改变；请重新输入保存。',
  'credential.store_unavailable':
    'Windows 凭据管理器拒绝了写入，API Key 没有保存；请稍后重新输入保存。',
  'models.configuration.revision_conflict':
    '模型设置刚有变化，API Key 没有保存；请关闭后重新打开这个连接，再输入保存。',
  'models.endpoint.not_found': '这个连接已经不存在，API Key 没有保存。',
};

/** Why a key slot that cannot take a new key refuses one, and that clearing it comes first. */
export function credentialStateRefusal(state: string): string {
  return state === 'reconciliation_required'
    ? '这个连接上次写入 API Key 没有确认完成，暂时不能保存新的，设置没有改变。请先点“清除”，再重新输入并保存。'
    : 'Windows 凭据管理器里读不到这个连接原来的 API Key，不能直接替换，设置没有改变。请先点“清除”，再重新输入并保存。';
}

/** A key that was not saved: why, and that the box it was typed in was emptied. */
export function saveCredentialFailure(error: unknown): string {
  if (answerMissing(error))
    return '没有等到 Aura Core 的答复，无法确认 API Key 是否已保存。请刷新模型设置查看，仍是未配置时重新输入保存。';
  const code = codeOf(error) ?? '';
  if (code === 'credential.reconciliation_required')
    return credentialStateRefusal('reconciliation_required');
  const known = CREDENTIAL_SAVE_REFUSALS[code];
  if (known !== undefined) return known;
  return `${userMessage(error, 'API Key 未保存，配置没有改变。')}输入框已清空，请重新输入 API Key 再保存。`;
}

/** A key that was not cleared, which the connection still uses. */
export function clearCredentialFailure(error: unknown): string {
  if (codeOf(error) === 'credential.store_unavailable')
    return 'Windows 凭据管理器拒绝了删除，API Key 仍然保存着，这个连接照常使用它。请稍后再点清除。';
  if (answerMissing(error))
    return '没有等到 Aura Core 的答复，无法确认 API Key 是否已清除。请刷新模型设置查看。';
  if (isConflict(error))
    return 'API Key 的状态刚有变化，这次没有清除；请关闭后重新打开这个连接再试。';
  return userMessage(error, 'API Key 没有清除，这个连接照常使用它；请稍后再点清除。');
}

export const CREDENTIAL_CLEARED = 'API Key 已清除，这个连接不再发送密钥。';

/**
 * Connections saved one after another, stopped at `failed`: those saved before it, why it was
 * not saved, and the ones after it that were not tried.
 */
export function routeSaveFailure(
  created: readonly string[],
  failed: string,
  untried: readonly string[],
  error: unknown,
): string {
  const saved = created.length > 0 ? `已保存 ${created.join('、')}；` : '';
  const after = untried.length > 0 ? `，之后的 ${untried.join('、')} 没有尝试` : '';
  if (answerMissing(error))
    return `${saved}${failed} 是否已保存没有确认（${connectionCause(error)}）${after}。请先刷新模型设置核对，再决定是否重新保存。`;
  const reason = connectionCause(error) ?? said(error);
  return `${saved}${failed} 没有保存${reason ? `（${reason}）` : ''}${after}。请检查 ${failed} 后再保存${
    untried.length > 0 ? '剩下的连接' : '一次'
  }。`;
}

/** The compatibility adjustments Aura learned for a connection, in words. */
export const COMPATIBILITY_TEXT = {
  heading: '兼容记录',
  reset: '重置',
  none: '这个连接还没有记下兼容调整。',
  unread: '兼容记录没有读到，现在看不出有没有记录；请关闭后重新打开这个连接。',
  resetFailed: '兼容记录未重置',
  resetDone: '兼容记录已重置',
  cleared: '这个连接记下的兼容调整已清除。',
  nothing: '这个连接没有记下兼容调整，无需重置。',
} as const;

/** The catalog's own issue codes as causes. */
const CATALOG_CAUSES: Readonly<Record<string, string>> = {
  'provider.catalog_refresh_failed': '连不上 models.dev',
  'provider.catalog_invalid': 'models.dev 返回的目录无法识别',
  'provider.catalog_response_too_large': 'models.dev 返回的目录过大',
  'persistence.catalog_cache_failed': '本机的目录缓存读写失败',
};
/** The catalog's code for a refresh that did not reach models.dev or got no usable answer. */
const CATALOG_NOT_REACHED = 'provider.catalog_refresh_failed';

const catalogCause = (code: string | null | undefined): string | undefined =>
  code ? CATALOG_CAUSES[code] : undefined;

/** A refresh that left the last catalog in place, by the catalog's issue code. */
export function catalogNotRefreshed(code: string | null | undefined): string {
  const cause = catalogCause(code);
  const network = code === CATALOG_NOT_REACHED || cause === undefined;
  return `models.dev 目录没有刷新${cause ? `（${cause}）` : ''}，模型列表仍是上次的。${
    network ? '请检查网络或代理后再刷新' : '请稍后再刷新'
  }。`;
}

/** A refresh that Core refused or did not answer. */
export function catalogNotRefreshedBy(error: unknown): string {
  const cause = connectionCause(error) ?? said(error);
  return `models.dev 目录没有刷新${cause ? `（${cause}）` : ''}；请稍后再刷新。`;
}

/** No catalog to show: by the catalog's issue code, or by the request that failed. */
export function catalogUnavailableText(code: string | null | undefined, error?: unknown): string {
  const cause = error === undefined ? catalogCause(code) : (connectionCause(error) ?? said(error));
  const network = error === undefined && (code === CATALOG_NOT_REACHED || cause === undefined);
  return `models.dev 目录暂不可用${cause ? `（${cause}）` : ''}，服务商列表无法显示。${
    network ? '请检查网络或代理后刷新目录' : '请稍后刷新目录'
  }。`;
}

export const CATALOG_ENTRY_MISSING =
  'models.dev 目录里找不到这个模型 ID，目录信息没有更新，设置不变。请确认模型 ID 是否写对。';
export const CATALOG_ENTRY_UPDATED = '已按目录更新这个模型的信息；你改过的设置和备注不变。';

/** A page of a Provider's catalog models that was not read. */
export function catalogPageFailure(error: unknown): string {
  const cause = connectionCause(error) ?? said(error);
  return `目录模型没有读到${cause ? `（${cause}）` : ''}，请稍后点“搜索”重试。`;
}
