import type {
  ToolCallActivitySummaryView,
  ToolNotExecutedReasonV1,
} from '@aura/contracts/generated';

/**
 * The kind a recorded error's own code names, where its class alone would say less: Chrome that
 * did not answer can be tried again, a tab in the background cannot be captured, and a memory
 * note changed since it was read was not written. A tool that is really unavailable keeps the
 * class's label. `tool` is the call's tool name, for labels that differ by tool.
 */
export const errorCodeLabel = (
  code: string | null | undefined,
  tool?: string,
): string | undefined => {
  switch (code) {
    case 'chrome.background_tab':
    case 'chrome.not_drawn':
      return '未执行：标签页在后台';
    // The user's own hand, or another conversation, kept the desktop; nothing was done.
    case 'computer.taken_over':
      return '你已接管，未执行';
    case 'computer.busy':
      return '电脑被占用，未执行';
    case 'computer.stopped':
      return '已停止，未执行';
    case 'computer.cancelled':
    case 'chrome.cancelled':
    case 'chrome.stopped':
      return '已取消，未执行';
    case 'computer.timeout':
    case 'chrome.timed_out':
      return '超时，未执行';
    case 'chrome.disconnected':
      return 'Chrome 已断开，未执行';
    case 'fs.file_in_use':
      return '文件被别的程序占用，未执行';
    case 'fs.deferred_by_editor_activity':
    case 'workspace.editor_reserved':
      return '文件正在编辑，未写入';
    case 'memory.secret_refused':
      return '含密钥，未写入';
    case 'git.repository_changed':
      return '仓库已变化，未执行';
    case 'tool.runtime.io_limit_exceeded':
      return '超过大小上限';
    case 'tool.runtime.http_gateway_failed':
      return '网络请求没有完成';
    case 'tool.result.http_partial_response':
      return '仅收到分段内容，未保存';
    // Another call held the file the whole wait; this call did nothing.
    case 'agent.resource_wait_exhausted':
    case 'fs.resource_busy':
      return '文件被占用，未执行';
    case 'memory.hash_conflict':
      return '冲突，未写入';
    // The terminal had moved on from the prompt the secure input was asked at; nothing was typed.
    case 'terminal.sensitive.prompt_gone':
      return '提示已不在，未输入';
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
    // A request may have reached its target before the time ran out, so no retry is suggested.
    // Core's own limit before a request went out reads the same.
    case 'tool.network.timed_out':
    case 'tool.runtime.timeout':
      return tool === 'http.download' ? '下载超时' : '超时';
    default:
      return code?.endsWith('.timed_out') ? '超时，可重试' : undefined;
  }
};

/**
 * A not-executed call whose recorded code says more than its stored reason: an approval nobody
 * answered in time was not refused, and a call a timeout stopped before it ran did not meet an
 * unavailable tool.
 */
export const notExecutedCodeLabel = (
  code: string | null | undefined,
  tool?: string,
): string | undefined => {
  switch (code) {
    case 'tool.approval.expired':
      return '批准已过期，未执行';
    case 'tool.approval.denied':
      return '你拒绝了，未执行';
    case 'tool.approval.invalidated':
      return '权限模式已变，未执行';
    case 'tool.hook.blocked':
      return '被 Hook 拦下，未执行';
    case 'tool.network.timed_out':
    case 'tool.runtime.timeout':
      return tool === 'http.download' ? '下载超时，未执行' : '超时，未执行';
    default:
      return undefined;
  }
};

/** A call whose turn was stopped in the moment after it was marked started: nothing of it was
 *  sent, so it did not run. */
export const endedBeforeStart = (call: ToolCallActivitySummaryView): boolean =>
  call.outcome?.type === 'cancelled' &&
  call.outcome.reason === 'turn_interrupted' &&
  (call.side_effect_state === 'no_effect' || call.side_effect_state === 'not_started');

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
        return (
          errorCodeLabel(call.error_code, call.display_name) ??
          `调用已完成 · ${rejectedReasonLabel(call)}`
        );
      case 'failed':
        return errorCodeLabel(call.error_code, call.display_name) ?? failedReasonLabel(call);
      case 'cancelled':
        if (endedBeforeStart(call)) return '未执行 · 会话中断';
        return call.outcome.reason === 'timeout' ? '已中断 · 超时' : '已中断 · 未正常完成';
      case 'not_executed':
        return (
          notExecutedCodeLabel(call.error_code, call.display_name) ??
          notExecutedReasonLabel(call.outcome.reason)
        );
      case 'indeterminate':
        return '结果未知';
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
      return '等待你批准';
    case 'waiting_resource':
      return '等待中';
    case 'queued':
      return '已排队';
    case 'running':
      return '运行中';
    case 'cancelling':
      return '正在取消';
    case 'finalized':
      return '已完成';
    // Its run ended before the call had a result; nothing works on it any more.
    case 'recovery_required':
      return '已中断，结果未知';
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
      return '未执行 · 权限拒绝';
    case 'approval_denied':
      return '未执行 · 你拒绝了';
    case 'runtime_unavailable':
      return '未执行 · 工具暂不可用';
    default: {
      const why = notExecutedReasonText(reason);
      return why === undefined ? '未执行' : `未执行 · ${why}`;
    }
  }
};

/**
 * Why a call was not run, in words, for every reason Core records; none for a row that names no
 * reason or one this build does not know.
 */
export const notExecutedReasonText = (
  reason: ToolNotExecutedReasonV1 | null | undefined,
): string | undefined => {
  switch (reason) {
    case 'purpose_required':
      return '缺少调用说明';
    case 'schema_validation_failed':
      return '参数错误';
    case 'resource_claim_resolution_failed':
      return '目标无法确定';
    case 'dependency_not_executed':
      return '依赖的调用未执行';
    case 'atomic_group_not_executed':
      return '同组调用未执行';
    case 'permission_denied':
      return '权限拒绝';
    case 'approval_denied':
      return '你拒绝了';
    case 'runtime_unavailable':
      return '工具暂不可用';
    case 'cancelled_before_dispatch':
      return '会话中断';
    case 'tool_not_found':
      return '没有这个工具';
    case 'not_loaded':
      return '工具未加载';
    case 'disabled':
      return '工具已停用';
    case 'trust_required':
      return '工具来源待信任';
    case 'audience_denied':
      return '这个对话不能用这个工具';
    case 'version_mismatch':
      return '工具已更新';
    case 'lineage_reconciliation':
      return '同一回答的其余部分没能继续';
    case 'completion_review':
      return '本轮正在结束';
    case 'batch_rejected':
      return '同批调用被拒绝';
    default:
      return undefined;
  }
};
