import type {
  DormantEndpointCredentialSummary,
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
      return '凭据不可用';
    case 'reconciliation_required':
      return '凭据需修复';
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
      return 'API Key：当前不可用';
    case 'reconciliation_required':
      return 'API Key：需要凭据恢复';
    default:
      return 'API Key：未配置';
  }
};
