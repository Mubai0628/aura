import type {
  AgentToolActivityEntry,
  ToolCallActivitySummaryView,
} from '@aura/contracts/generated';
import {
  endedBeforeStart,
  errorCodeLabel,
  notExecutedCodeLabel,
  notExecutedReasonText,
  toolCallStateLabel,
} from '../tool-call-state-label';

/** The real visual tone of one Tool call; text always states the same thing. */
export type ToolCallTone = 'success' | 'failed' | 'waiting' | 'running' | 'unknown';

/**
 * The reason a terminal call ended, in the words every surface shows.
 *
 * It reads the projection's own classification — the stored error's category and the
 * not-executed reason — and never the message text. That is the whole point: four
 * `fs.not_found` refusals were displayed as 已拒绝 because the renderer keyed on the
 * broad outcome alone, and the same mistake in reverse (reading English text to guess a
 * permission problem) is what this function must not do.
 */
export const terminalReasonLabel = (summary: ToolCallActivitySummaryView): string | undefined => {
  // An unconfirmed side effect is a claim about what happened, not a kind of failure, so
  // it outranks every error text below: a row must never present an operation whose
  // effect is unknown as an ordinary refusal or an ordinary success.
  if (summary.side_effect_state === 'unknown') return '结果未知';
  const outcome = summary.outcome;
  if (outcome === undefined || outcome === null) return undefined;
  if (outcome.type === 'not_executed') {
    const named = notExecutedCodeLabel(summary.error_code, summary.display_name);
    if (named !== undefined) return named;
    switch (outcome.reason) {
      case 'purpose_required':
        return '缺少调用说明，未执行';
      case 'schema_validation_failed':
        return '参数错误，未执行';
      case 'resource_claim_resolution_failed':
        return '目标无法确定，未执行';
      case 'dependency_not_executed':
        return '依赖未执行';
      case 'atomic_group_not_executed':
        return '同组调用未执行';
      case 'permission_denied':
        return '权限拒绝，未执行';
      case 'approval_denied':
        return '你拒绝了，未执行';
      case 'runtime_unavailable':
        return notStartedLabel(summary);
      default: {
        const why = notExecutedReasonText(outcome.reason);
        return why === undefined ? '未执行' : `${why}，未执行`;
      }
    }
  }
  if (outcome.type === 'indeterminate') return '结果未知';
  if (outcome.type === 'cancelled') {
    if (endedBeforeStart(summary)) return '会话中断，未执行';
    switch (outcome.reason) {
      case 'timeout':
        return '超时';
      case 'user_requested':
      case 'parent_agent_cancelled':
      case 'batch_cancelled':
        return '已停止';
      case 'turn_interrupted':
      case 'core_shutdown':
      case 'permission_revoked':
      case 'scope_closed':
        return '已中断';
      default:
        return '已取消';
    }
  }
  if (outcome.type === 'succeeded') return '成功';
  // A rejected or failed invocation is described by the kind its own code names, else by the
  // class the Store recorded. An absent class is a legacy row, and it stays neutral rather than
  // being guessed at.
  const named = errorCodeLabel(summary.error_code, summary.display_name);
  if (named !== undefined) return named;
  switch (summary.error_class) {
    case 'not_found':
      return '目标不存在';
    case 'permission':
      return '权限拒绝';
    case 'conflict':
      return '状态已变化';
    case 'invalid_request':
      return outcome.type === 'operation_rejected' ? '请求无效，未执行' : '执行失败：请求无效';
    // Every failure a backend reports while running is stored under one general code in the
    // unavailable class; only a tool that is really unavailable is called so.
    case 'unavailable':
      return summary.error_code === 'tool.runtime.dispatch_failed' ? '执行失败' : '工具暂不可用';
    default:
      return outcome.type === 'operation_rejected' ? '已拒绝' : '执行失败';
  }
};

/**
 * A call Core stopped before it started. Core stores these under the general dispatch code with
 * the class of their cause: a stopped turn as cancelled, a refusal before start (a sub-agent's
 * main agent, a desktop notice, a network check) as a conflict, a permission rule as permission.
 */
const notStartedLabel = (summary: ToolCallActivitySummaryView): string => {
  if (summary.error_code !== 'tool.runtime.dispatch_failed') return '工具暂不可用，未执行';
  switch (summary.error_class) {
    case 'other':
      return '会话中断，未执行';
    case 'conflict':
      return '开始前被拦下，未执行';
    case 'permission':
      return '权限拒绝，未执行';
    default:
      return '工具暂不可用，未执行';
  }
};

export const toolCallTone = (entry: AgentToolActivityEntry | undefined): ToolCallTone => {
  if (!entry) return 'unknown';
  const { summary } = entry;
  if (summary.side_effect_state === 'unknown') return 'unknown';
  switch (summary.outcome?.type) {
    case 'succeeded':
      return 'success';
    case 'cancelled':
      return endedBeforeStart(summary) ? 'unknown' : 'failed';
    case 'failed':
    case 'operation_rejected':
      return 'failed';
    case 'not_executed':
      return 'unknown';
    case 'indeterminate':
      return 'unknown';
    default:
      break;
  }
  switch (summary.status) {
    case 'waiting_permission':
    case 'waiting_approval':
    case 'waiting_dependency':
    case 'waiting_resource':
    case 'pending_admission':
      return 'waiting';
    case 'recovery_required':
      return 'unknown';
    default:
      return 'running';
  }
};

/** The state text shown next to the chip, including the not-yet-read case. */
export const toolCallState = (entry: AgentToolActivityEntry | undefined): string => {
  if (!entry) return '状态未在本页记录';
  const { summary } = entry;
  if (summary.side_effect_state === 'unknown') return '结果未知';
  const reason = terminalReasonLabel(summary);
  if (reason !== undefined) return reason;
  return toolCallStateLabel(summary);
};

/** Stable Tool identity for grouping: the exact definition, not the label. */
export const toolCallIdentity = (entry: AgentToolActivityEntry): string => {
  const { tool_id, tool_version, tool_call_id, display_name } = entry.summary;
  if (
    tool_id === null ||
    tool_id === undefined ||
    tool_version === null ||
    tool_version === undefined
  ) {
    return `unbound:${tool_call_id}`;
  }
  return `${tool_id}@${tool_version.definition_revision}/${tool_version.semantic_version ?? ''}#${display_name}`;
};

/**
 * The exact real state, for deciding whether two calls may share a group.
 *
 * This is deliberately finer than the visual tone: a queued call and a running
 * call look alike in colour but are different facts, so they never merge. A terminal
 * outcome carries its *reason* as well as its kind, because `operation_rejected` with
 * `not_found` and `operation_rejected` with `permission` are two different things a
 * reader has to be able to tell apart.
 */
export const toolCallStateKey = (entry: AgentToolActivityEntry): string => {
  const { summary } = entry;
  if (summary.side_effect_state === 'unknown') return 'indeterminate';
  if (summary.outcome !== undefined && summary.outcome !== null)
    return `outcome:${summary.outcome.type}:${terminalReasonKey(summary)}`;
  return `status:${summary.status}`;
};

/**
 * The final state two calls must share before they may collapse into one run, or
 * `undefined` while the call has no explicit final state yet.
 *
 * A queued, running or waiting call is still an open fact: it is always shown on
 * its own and only groups after the real outcome exists. An unknown side-effect
 * state never collapses either, so a group never claims a result nobody recorded.
 */
export const toolCallFinalStateKey = (entry: AgentToolActivityEntry): string | undefined => {
  const { summary } = entry;
  if (summary.side_effect_state === 'unknown') return undefined;
  const outcome = summary.outcome;
  if (outcome === undefined || outcome === null) return undefined;
  if (summary.status !== 'finalized') return undefined;
  return `outcome:${outcome.type}:${terminalReasonKey(summary)}`;
};

/** The stable part of a terminal reason, for keys rather than for display. */
const terminalReasonKey = (summary: ToolCallActivitySummaryView): string => {
  const outcome = summary.outcome;
  if (outcome === undefined || outcome === null) return 'none';
  if (outcome.type === 'not_executed') return `reason:${outcome.reason}`;
  if (outcome.type === 'indeterminate') return `reason:${outcome.reason}`;
  if (outcome.type === 'cancelled') return `reason:${outcome.reason}`;
  if (errorCodeLabel(summary.error_code, summary.display_name) !== undefined)
    return `code:${summary.error_code}`;
  return `class:${summary.error_class ?? 'unclassified'}`;
};
