import type { RecoveryOutcome } from '@aura/contracts/generated';
import { connectionCause, refusedAction } from '../agent-session/failure-text';

/**
 * What settings recovery says in words: what a restore has to be linked to again, why a preview
 * or a recovery did not happen, and what a recovery that only partly applied left.
 */

const unique = (values: readonly string[]): string[] => [...new Set(values)];

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** What each cross-domain reference key Core names points at. */
const REFERENCES: Readonly<Record<string, string>> = {
  default_model_profile_id: '默认模型',
  default_provider_id: '默认服务商',
  default_endpoint_id: '默认连接',
  credential_id: '凭据',
  credential_revision: '凭据',
  fixed_tool_ids: '固定的工具',
  model_profile_id: '模型',
  provider_id: '服务商',
  endpoint_id: '连接',
  servers: 'MCP 服务器',
  workspace_id: '工作区',
  session_id: '会话',
};

/** The settings a restore has to be linked to again, in words, each once. */
export const referenceList = (keys: readonly string[]): string =>
  unique(keys.map((key) => REFERENCES[key] ?? '其他关联设置')).join('、');

/** A recovery Core refused before writing, by its code. */
const APPLY_REFUSALS: Readonly<Record<string, string>> = {
  'config.recovery.action_not_previewed': '这项恢复还没有预览，没有执行；请先预览再恢复。',
  'config.recovery.apply_failed': '恢复没有写入，设置保持原样；请重新预览后再试。',
  'config.recovery.apply_boundary_refused':
    '恢复没有开始（配置写入正忙），设置保持原样；请重新预览后再试。',
};

export const applyRefusal = (code: string): string | undefined => APPLY_REFUSALS[code];

/** Why a preview was not made, by Core's code. */
const PREVIEW_CAUSES: Readonly<Record<string, string>> = {
  'config.recovery.plan_boundary_refused': '配置读取正忙',
  'config.recovery.plan_boundary_unknown': '配置读取没有及时完成',
};

/** A preview that was not made: why, and that nothing was recovered. */
export function previewFailure(error: unknown): string {
  const code = codeOf(error);
  const cause = (code === undefined ? undefined : PREVIEW_CAUSES[code]) ?? connectionCause(error);
  return cause === undefined
    ? refusedAction(error, '无法生成恢复预览', '没有执行恢复；请稍后重试。')
    : `无法生成恢复预览（${cause}），没有执行恢复；请稍后重试。`;
}

/** A recovery that only partly applied: what did not recover and why, and what to do. */
export function partlyRecovered(
  outcomes: readonly RecoveryOutcome[],
  label: (module: string) => string,
  reasons: Readonly<Record<string, string>>,
): string {
  const failed = outcomes.filter((outcome) => !outcome.applied);
  const names = unique(failed.map((outcome) => label(outcome.module)));
  const why = unique(
    failed.flatMap((outcome) => {
      const reason = outcome.reason_code ? reasons[outcome.reason_code] : undefined;
      return reason === undefined ? [] : [reason];
    }),
  ).join('；');
  const rest = outcomes.some((outcome) => outcome.applied) ? '，其余已恢复' : '';
  return `${names.join('、')}没有恢复${why ? `（${why}）` : ''}${rest}。请对${
    names.length > 1 ? '这几项' : '这一项'
  }重新预览后再恢复。`;
}
