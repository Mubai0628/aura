import type {
  ModelAvailabilityCheckResult,
  ModelDiscoveryRunView,
  ModelDiscoveryState,
  ModelOperationCancellationReason,
} from '@aura/contracts/generated';

/**
 * How a model list request or an availability test went, in words: what each check found, how a
 * request ended with its counts, and why one was cancelled.
 */

/** The checks' own failures in words; an unlisted code is described by its family. */
const CHECK_FAILURES: Record<string, string> = {
  'authentication.rejected': 'API Key 被拒绝，请检查后重新保存',
  'transport.authorization_failed': '认证没有通过，请检查 API Key',
  'provider.rate_limited': '服务商限流，请稍后再试',
  'provider.server_error': '服务商出错，请稍后再试',
  'provider.request_rejected': '服务商拒绝了请求，请检查模型名称与连接设置',
  // A setting of the user's account at the Provider, which accepted the key.
  'provider.account_region':
    '服务商账号的区域设置不允许这个模型，请在服务商那边开放对应区域，或换一个模型',
  'provider.account_data_policy':
    '服务商账号的隐私设置不允许这个模型的服务节点，请在服务商那边修改，或换一个模型',
  'provider.client_not_allowed':
    '这个套餐只允许名单上的客户端，拒绝了 Aura，请试这家服务商其他协议的连接，或改用普通 API',
  'provider.invalid_response': '服务商的回复无法识别',
  'provider.visible_text_missing': '回复里没有可显示的文字',
  'network.timeout': '连接超时，请检查网络与代理设置',
  'network.transport_failed': '连接失败，请检查网络与代理设置',
  'network.proxy_unauthorized': '代理要求认证，请检查代理设置',
  'network.proxy_interrupted': '代理连接中断，请检查代理设置',
  'network.proxy_protocol_error': '代理回复异常，请检查代理设置',
  'network.tls_server_name_mismatch': '证书与地址不符，请检查 Base URL',
  'network.destination_blocked': '这个地址被网络规则拦截',
  'network.invalid_url': '地址无效，请检查 Base URL',
};
const CHECK_NOT_RUN: Record<string, string> = {
  blocked_by_previous_stage: '前一项没有通过',
  not_applicable_for_scheme: '不适用于这种连接方式',
  no_authentication_required: '这个连接无需认证',
  authentication_not_confirmed: '认证没有得到确认',
};

export const checkFailure = (code: string, status: number | null | undefined): string => {
  const words =
    CHECK_FAILURES[code] ??
    (code.startsWith('network.')
      ? '网络请求失败，请检查网络与代理设置'
      : code.startsWith('provider.')
        ? '服务商返回了错误'
        : '这一项没有通过');
  return status === null || status === undefined ? words : `${words}（HTTP ${status}）`;
};

export const availabilityCheckLabel = (result: ModelAvailabilityCheckResult): string => {
  switch (result.status) {
    case 'passed':
      return result.latency_ms === null || result.latency_ms === undefined
        ? '通过'
        : `通过 · ${result.latency_ms} ms`;
    case 'failed':
      return `失败 · ${checkFailure(result.error_code, result.provider_status)}`;
    case 'not_run':
      return `未执行 · ${CHECK_NOT_RUN[result.reason_code] ?? '没有执行'}`;
  }
};

const DISCOVERY_CANCELLED: Partial<Record<ModelOperationCancellationReason, string>> = {
  configuration_changed: '连接设置已改变，请重新获取',
  application_restarted: 'Aura 重启过，请重新获取',
  target_deleted: '这个连接已删除',
};

const partialCounts = (run: ModelDiscoveryRunView): string => {
  const parts: string[] = [];
  if (typeof run.returned_count === 'number') parts.push(`返回 ${run.returned_count} 个`);
  if (typeof run.created_count === 'number') parts.push(`新增 ${run.created_count} 个`);
  if (run.not_returned_count) parts.push(`${run.not_returned_count} 个这次没有返回`);
  return parts.length > 0 ? `部分完成：${parts.join('，')}` : '部分完成';
};

/** Where a model list request is, or how it ended: with its counts, or why. */
export function discoveryOutcome(
  state: ModelDiscoveryState,
  run: ModelDiscoveryRunView | undefined,
): string {
  if (state === 'cancelled') {
    const reason = run?.cancellation_reason
      ? DISCOVERY_CANCELLED[run.cancellation_reason]
      : undefined;
    return reason === undefined ? '已取消' : `已取消：${reason}`;
  }
  if (state !== 'completed') return state === 'queued' ? '排队中' : '请求中';
  switch (run?.outcome) {
    case 'succeeded':
      return '已完成';
    case 'partial':
      return partialCounts(run);
    case 'unsupported':
      return '当前配置已禁用';
    default:
      // A failure with no reply from the Provider has no Provider error; one with a reply shows
      // that error below the outcome.
      return run?.provider_error
        ? '失败：服务商返回了错误，请检查连接后重试'
        : '失败：没有收到服务商的有效回复，请检查网络、代理和 Base URL 后重试';
  }
}

const TEST_CANCELLED: Partial<Record<ModelOperationCancellationReason, string>> = {
  configuration_changed: '连接设置已改变。保存后重新测试。',
  application_restarted: 'Aura 重启过，测试没有做完。需要时重新测试。',
  target_deleted: '这个模型或它的连接已删除。',
};

/** An availability test of `name` that was cancelled, and why when it was not the user. */
export function availabilityCancelled(
  name: string,
  reason: ModelOperationCancellationReason | null | undefined,
): string {
  const why = reason ? TEST_CANCELLED[reason] : undefined;
  return why === undefined ? `${name} 的可用性测试已取消。` : `${name} 的可用性测试已取消：${why}`;
}
