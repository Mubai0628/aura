import type {
  ToolCallActivitySummaryView,
  ToolNotExecutedReasonV1,
} from '@aura/contracts/generated';

/**
 * The longer label used by the shared state surfaces.
 *
 * It classifies exactly like the row and the chip do — from the projection's error
 * class and not-executed reason, never from the message text — so one call cannot be
 * described as a refusal in a list and as a permission problem in a detail view.
 */
export const toolCallStateLabel = (call: ToolCallActivitySummaryView): string => {
  if (call.side_effect_state === 'unknown') return '结果未知';
  if (call.outcome !== undefined && call.outcome !== null) {
    switch (call.outcome.type) {
      case 'succeeded':
        return '调用已完成';
      case 'operation_rejected':
        return `调用已完成 · ${rejectedReasonLabel(call)}`;
      case 'failed':
        return failedReasonLabel(call);
      case 'cancelled':
        return '已中断 · 未正常完成';
      case 'not_executed':
        return notExecutedReasonLabel(call.outcome.reason);
      case 'indeterminate':
        return `结果不确定 · ${call.outcome.reason}`;
    }
  }
  switch (call.status) {
    case 'pending_admission':
      return '等待接纳';
    case 'waiting_dependency':
      return '等待依赖';
    case 'waiting_permission':
      return '等待权限';
    case 'waiting_approval':
      return '等待审批';
    case 'waiting_resource':
      return '等待资源';
    case 'queued':
      return '已排队';
    case 'running':
      return '运行中';
    case 'cancelling':
      return '正在取消';
    case 'finalized':
      return '已完成';
    case 'recovery_required':
      return '需要恢复';
  }
};

const rejectedReasonLabel = (call: ToolCallActivitySummaryView): string => {
  switch (call.error_class) {
    case 'not_found':
      return '目标不存在';
    case 'permission':
      return '权限拒绝';
    case 'conflict':
      return '状态已变化';
    case 'invalid_request':
      return '请求无效';
    case 'unavailable':
      return '工具暂不可用';
    default:
      return '操作被拒绝';
  }
};

const failedReasonLabel = (call: ToolCallActivitySummaryView): string => {
  switch (call.error_class) {
    case 'unavailable':
      return '工具调用失败 · 工具暂不可用';
    case 'not_found':
      return '工具调用失败 · 目标不存在';
    default:
      return '工具调用失败';
  }
};

const notExecutedReasonLabel = (reason: ToolNotExecutedReasonV1): string => {
  switch (reason) {
    case 'purpose_required':
      return '未执行 · 缺少调用说明';
    case 'schema_validation_failed':
      return '未执行 · 参数错误';
    case 'resource_claim_resolution_failed':
      return '未执行 · 目标无法确定';
    case 'permission_denied':
    case 'approval_denied':
      return '未执行 · 权限拒绝';
    case 'runtime_unavailable':
      return '未执行 · 工具暂不可用';
    default:
      return `未执行 · ${reason}`;
  }
};
