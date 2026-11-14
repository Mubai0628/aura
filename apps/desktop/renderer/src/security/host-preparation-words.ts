import type {
  HostPreparationAction,
  HostPreparationOperationView,
} from '@aura/contracts/generated';
import { HOST_ACTION_TITLE } from '../../../shared/host-preparation-text';
import { answerMissing, connectionCause, refusedAction } from '../agent-session/failure-text';

/**
 * What a compatibility environment request that did not go through says, and where an operation
 * under way or ended is: whether Windows was changed, why, and what to press next.
 */

const codeOf = (error: unknown): string | undefined => {
  if (typeof error !== 'object' || error === null) return undefined;
  const code = (error as { code?: unknown }).code;
  return typeof code === 'string' ? code : undefined;
};

/** Why an operation ended without its change, by Core's code; each says whether Windows changed. */
const FAILED_BECAUSE: Readonly<Record<string, string>> = {
  'sandbox.host_preparation_capability_conflict':
    '系统里现有的权限设置和这份方案冲突，系统没有被修改',
  'sandbox.host_preparation_plan_invalid': '准备方案和系统当前的状态对不上，系统没有被修改',
  'sandbox.host_preparation_plan_expired': '准备方案已过期，系统没有被修改',
  'sandbox.host_preparation_release_changed': '安装的 Aura 变了，这份方案作废，系统没有被修改',
  'sandbox.host_preparation_release_trust_failed':
    '这个 Aura 安装包没有通过签名校验，系统没有被修改',
  'sandbox.host_preparation_uac_failed': '管理员权限请求没有成功，系统没有被修改',
  'sandbox.elevated_state_unknown':
    '没有确认是否完成，系统可能已被部分修改；请点“修复兼容环境”重新检查',
};

/** A request Core refused for a reason it names, in a sentence; `button` is the action pressed. */
function refusedBecause(code: string | undefined, button: string): string | undefined {
  switch (code) {
    case 'sandbox.host_preparation_release_trust_failed':
      return '这个 Aura 安装包没有通过签名校验，所以没有请求管理员权限，系统没有被修改。请从官方发布页重新下载安装。';
    case 'sandbox.host_preparation_busy':
    case 'sandbox.host_preparation_operation_active':
      return '另一项兼容环境检查或操作还在进行，这次没有开始；等它结束再试。';
    case 'sandbox.host_preparation_plan_expired':
    case 'sandbox.host_preparation_plan_invalid':
    case 'sandbox.host_preparation_release_changed':
      return `${FAILED_BECAUSE[code]}；请重新点“${button}”。`;
    case 'sandbox.host_preparation_plan_stale':
    case 'sandbox.host_preparation_revision_conflict':
      return `准备期间兼容环境的状态又变了，系统没有被修改；请重新点“${button}”。`;
    case 'sandbox.host_preparation_capability_conflict':
      return `${FAILED_BECAUSE[code]}。`;
    case 'sandbox.host_preparation_uac_failed':
      return `${FAILED_BECAUSE[code]}；可以再点“${button}”重试。`;
    case 'sandbox.elevated_state_unknown':
      return `${FAILED_BECAUSE[code]}。`;
    case 'sandbox.host_preparation_probe_failed':
    case 'sandbox.host_preparation_unavailable':
      return '没能检查系统的兼容状态，这次没有开始，系统没有被修改；请稍后再试。';
    case 'sandbox.host_preparation_identity_unverified':
      return '没能核验已安装的 Aura，无法决定是否修改空设备 NUL，这次没有开始，系统没有被修改；请稍后再试，一直这样请重新安装 Aura。';
    case 'sandbox.host_preparation_ownership_conflict':
      return 'Aura 记录的改动属于另一个系统盘，和现在的系统盘对不上，这次没有开始，系统没有被修改。';
    case 'sandbox.host_preparation_denied':
      return '你拒绝了管理员权限，没有做任何更改。';
    default:
      return undefined;
  }
}

const CHECK_STATE = '请看“兼容环境”一项的状态，核对后再决定是否重试。';

/**
 * A prepare, repair or remove request that did not go through: why, whether Windows was changed,
 * and what to press next. One whose answer did not come may have started.
 */
export function hostRequestFailure(error: unknown, action: HostPreparationAction): string {
  const button = HOST_ACTION_TITLE[action];
  const known = refusedBecause(codeOf(error), button);
  if (known !== undefined) return known;
  const cause = connectionCause(error);
  if (answerMissing(error))
    return `没有等到 Aura Core 的答复${cause ? `（${cause}）` : ''}，${button}可能已经开始；${CHECK_STATE}`;
  if (cause !== undefined) return `${button}没有开始（${cause}），系统没有被修改；请稍后再试。`;
  return refusedAction(error, `${button}没有完成`, CHECK_STATE);
}

const STATE_TEXT: Record<Exclude<HostPreparationOperationView['state'], 'failed'>, string> = {
  prepared: '方案已准备好，等待确认',
  awaiting_elevation: '等待你在系统窗口里确认管理员权限',
  applying: '正在应用',
  completed: '已完成',
  rollback_pending: '正在撤回',
  rolled_back: '已撤回，系统恢复原样',
  denied: '你拒绝了管理员权限，没有做任何更改',
};

/** Where an operation under way or ended is, in words; a failed one says why. */
export function hostOperationState(
  operation: Pick<HostPreparationOperationView, 'state' | 'error_code'>,
): string {
  if (operation.state !== 'failed') return STATE_TEXT[operation.state];
  const reason =
    operation.error_code === null || operation.error_code === undefined
      ? undefined
      : FAILED_BECAUSE[operation.error_code];
  return `失败：${reason ?? '没有完成'}`;
}
