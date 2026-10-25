import type { IpcErrorEnvelope } from '@aura/contracts/generated';
import { userMessage } from '../agent-session/user-message';
import type {
  EndpointEditorField,
  EndpointEditorValidationIssue,
} from '../provider-connection-validation';

/** Core's endpoint refusals, mapped to the editor field they are about. */
export const endpointFieldIssueFromIpc = (
  error: IpcErrorEnvelope,
): EndpointEditorValidationIssue | undefined => {
  const fallbackByCode: Partial<Record<string, { field: EndpointEditorField; message: string }>> = {
    'models.endpoint.chat_instruction_role_required': {
      field: 'chat_instruction_role',
      message: 'OpenAI Chat Completions 必须选择 Chat Instruction Role',
    },
    'models.endpoint.chat_output_token_field_required': {
      field: 'chat_output_token_field',
      message: 'OpenAI Chat Completions 必须选择 Output Token Field',
    },
    'models.endpoint.invalid_auth_scheme': {
      field: 'auth_scheme',
      message: '当前认证方式不适用于所选 API 类型',
    },
    'models.endpoint.invalid_base_url': {
      field: 'base_url',
      message: 'Base URL 无效，请填写不含用户信息、查询参数或片段的有效地址',
    },
    'models.endpoint.invalid_display_name': {
      field: 'display_name',
      message: '请输入连接名称',
    },
    'models.endpoint.invalid_models_path': {
      field: 'models_path',
      message: '模型目录请求路径无效，请填写同一 Endpoint Origin 下的 HTTP 路径',
    },
    'models.endpoint.invalid_parallel_limit': {
      field: 'max_parallel_model_requests',
      message: 'Endpoint 并发上限必须是大于 0 的整数',
    },
    'models.endpoint.invalid_timeout': {
      field: 'connect_ms',
      message: '超时必须是大于 0 的整数',
    },
  };
  const fallback = fallbackByCode[error.code];
  if (
    fallback === undefined &&
    error.code !== 'models.endpoint.field_not_allowed' &&
    error.code !== 'models.endpoint.invalid_field'
  ) {
    return undefined;
  }
  const field = endpointEditorFieldFromCore(error.details.field) ?? fallback?.field;
  if (field === undefined) return undefined;
  return {
    field,
    message:
      error.code === 'models.endpoint.field_not_allowed'
        ? '该字段不适用于当前 API 类型，请核对后重新保存'
        : error.code === 'models.endpoint.invalid_field'
          ? '该字段值无效，请核对后重新保存'
          : (fallback?.message ?? userMessage(error, '该字段值无效，请核对后重新保存')),
  };
};

export const endpointEditorFieldFromCore = (field: unknown): EndpointEditorField | undefined => {
  switch (field) {
    case 'auth_scheme':
    case 'base_url':
    case 'chat_instruction_role':
    case 'chat_output_token_field':
    case 'connect_ms':
    case 'first_event_ms':
    case 'models_path':
    case 'stream_idle_ms':
    case 'total_ms':
      return field;
    case 'display_name':
    case 'endpoint.display_name':
      return 'display_name';
    case 'max_parallel_model_requests':
      return 'max_parallel_model_requests';
    default:
      return undefined;
  }
};
