import type {
  DormantEndpointCredentialSummary,
  EndpointCredentialState,
  EndpointCredentialStatus,
  ModelEndpointConfig,
} from '@aura/contracts/generated';

/** How an endpoint's API-key slot is described in the dialog. */
export const credentialStatusLabel = (
  endpoint: ModelEndpointConfig,
  status: EndpointCredentialStatus | undefined,
  dormant: DormantEndpointCredentialSummary | undefined,
): string => {
  if (endpoint.auth_scheme === 'none') {
    return dormant === undefined ? '无需凭据' : '凭据已保留，当前未使用';
  }
  switch (status?.state) {
    case 'configured':
      return '凭据已配置';
    case 'cleared':
      return '凭据已清除';
    case 'unavailable':
      return '凭据读不到';
    case 'reconciliation_required':
      return '凭据待核对';
    default:
      return '凭据未配置';
  }
};

export const credentialStatusTitle = (status: EndpointCredentialStatus | undefined): string => {
  switch (status?.state) {
    case 'configured':
      return 'API Key：已配置';
    case 'cleared':
      return 'API Key：已清除';
    case 'unavailable':
      return 'API Key：Windows 凭据管理器里读不到，请清除后重新保存';
    case 'reconciliation_required':
      return 'API Key：上次写入没有确认完成，暂时不能保存新的；可先清除再保存';
    default:
      return 'API Key：未配置';
  }
};

/** Why a connection's models cannot be tested yet: the key it lacks; none when they can be. */
export const availabilityPrerequisite = (
  authScheme: ModelEndpointConfig['auth_scheme'],
  state: EndpointCredentialState | undefined,
): string | undefined => {
  if (authScheme === 'none' || state === 'configured') return undefined;
  if (state === 'unavailable')
    return 'Windows 凭据管理器里读不到这个连接的 API Key，清除后重新保存才能测试。';
  if (state === 'reconciliation_required')
    return '这个连接上次写入 API Key 没有确认完成，清除后重新保存才能测试。';
  return '这个连接还没有 API Key，保存 API Key 后才能测试。';
};
