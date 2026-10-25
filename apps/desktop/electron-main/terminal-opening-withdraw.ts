import type { OutboundTask } from './outbound-send-owner.js';
import type {
  CorrelationId,
  CommandInvocationId,
  IpcErrorEnvelope,
} from '@aura/contracts/generated';
import { errorEnvelope, requestRecoveryEnvelope } from './request-failure-envelopes.js';
import { OUTCOME_RECOVERY_NOT_SENT } from '../shared/ipc-request-failure.js';
/** Only the caller-owned opening frame may be withdrawn, before submission. */
export function withdrawUnsentOpening(
  task: OutboundTask | undefined,
  submitted: boolean,
  finish: () => void,
): boolean {
  if (!task || submitted || task.outcome().wroteAnything || !task.withdraw()) return false;
  finish();
  return true;
}
export function createOpeningWithdrawal(
  id: CorrelationId,
  invocation: CommandInvocationId | undefined,
  task: () => OutboundTask | undefined,
  submitted: () => boolean,
  finish: (envelope: IpcErrorEnvelope) => void,
): () => boolean {
  return () =>
    withdrawUnsentOpening(task(), submitted(), () =>
      finish(
        requestRecoveryEnvelope(
          'terminal.user.open',
          invocation,
          errorEnvelope('request_cancelled', '已取消打开。', 'cancelled', id),
          OUTCOME_RECOVERY_NOT_SENT,
        ),
      ),
    );
}
