import type {
  ToolCallActivitySummaryView,
  ToolNotExecutedReasonV1,
} from '@aura/contracts/generated';

/**
 * The kind a recorded error's own code names, where its class alone would say less: Chrome that
 * did not answer can be tried again, a tab in the background cannot be captured, and a memory
 * note changed since it was read was not written. A tool that is really unavailable keeps the
 * class's label.
 */
export const errorCodeLabel = (code: string | null | undefined): string | undefined => {
  switch (code) {
    case 'chrome.background_tab':
    case 'chrome.not_drawn':
      return '未执行：标签页在后台';
    case 'memory.hash_conflict':
      return '冲突，未写入';
    // The target of an HTTP tool could not be reached; the details say which and what to do.
    case 'tool.network.connection_refused':
      return '连接被拒绝';
    case 'tool.network.host_not_found':
      return '找不到主机';
    case 'tool.network.tls_failed':
      return 'TLS 握手失败';
    case 'tool.network.connection_reset':
      return '连接被断开';
    case 'tool.network.connection_failed':
      return '连接失败';
    default:
      return code?.endsWith('.timed_out') ? '超时，可重试' : undefined;
  }
};

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
        return errorCodeLabel(call.error_code) ?? `调用已完成 · ${rejectedReasonLabel(call)}`;
      case 'failed':
        return errorCodeLabel(call.error_code) ?? failedReasonLabel(call);
      case 'cancelled':
        return call.outcome.reason === 'timeout' ? '已中断 · 超时' : '已中断 · 未正常完成';
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
