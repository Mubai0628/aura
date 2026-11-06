import type {
  EndpointAuthScheme,
  ModelApiFormat,
  ProviderPresetParameterView,
} from '@aura/contracts/generated';

import { parameterValueIsValid } from './provider-connection/base-url-template';

export type EndpointEditorField =
  | 'api_format'
  | 'auth_scheme'
  | 'base_url'
  | 'chat_instruction_role'
  | 'chat_output_token_field'
  | 'connect_ms'
  | 'display_name'
  | 'first_event_ms'
  | 'form'
  | 'max_parallel_model_requests'
  | 'models_path'
  | 'stream_idle_ms'
  | 'total_ms'
  | `parameter:${string}`;

export interface EndpointEditorValidationIssue {
  field: EndpointEditorField;
  message: string;
}

interface EndpointEditorValidationInput {
  apiFormat: ModelApiFormat | '';
  authScheme: EndpointAuthScheme;
  baseUrl: string;
  chatInstructionRole: '' | 'developer' | 'system';
  chatOutputTokenField: '' | 'max_completion_tokens' | 'max_tokens';
  connectMs: number;
  connectionName: string;
  discoveryMode: 'automatic' | 'custom' | 'disabled';
  firstEventMs: number;
  hasLocalProvider: boolean;
  maxParallelRequests: number;
  modelsPath: string;
  /** The values given for `urlParameters`, by name. */
  parameterValues?: Readonly<Record<string, string>>;
  streamIdleMs: number;
  totalMs: number | '';
  /** The values of the user's own the chosen address takes. */
  urlParameters?: readonly ProviderPresetParameterView[];
}

export const customModelsPathIssue = (
  discoveryMode: EndpointEditorValidationInput['discoveryMode'],
  modelsPath: string,
): string | undefined => {
  if (discoveryMode !== 'custom' || modelsPath.startsWith('/')) return undefined;
  return '模型目录请求路径必须以 / 开头，例如 /models';
};

const isPositiveInteger = (value: number): boolean => Number.isSafeInteger(value) && value > 0;

export const endpointEditorValidationIssues = ({
  apiFormat,
  authScheme,
  baseUrl,
  chatInstructionRole,
  chatOutputTokenField,
  connectMs,
  connectionName,
  discoveryMode,
  firstEventMs,
  hasLocalProvider,
  maxParallelRequests,
  modelsPath,
  parameterValues = {},
  streamIdleMs,
  totalMs,
  urlParameters = [],
}: EndpointEditorValidationInput): EndpointEditorValidationIssue[] => {
  const issues: EndpointEditorValidationIssue[] = [];
  if (!hasLocalProvider) {
    issues.push({ field: 'form', message: '请先创建可配置的本地 Provider' });
  }
  if (connectionName.trim().length === 0) {
    issues.push({ field: 'display_name', message: '请输入连接名称' });
  }
  if (apiFormat === '') {
    issues.push({ field: 'api_format', message: '请选择 API 类型' });
  }
  if (
    (apiFormat === 'open_ai_responses' || apiFormat === 'open_ai_chat_completions') &&
    authScheme === 'anthropic_api_key'
  ) {
    issues.push({
      field: 'auth_scheme',
      message: '所选 OpenAI API 类型不能使用 Anthropic API Key',
    });
  }
  if (apiFormat === 'anthropic_messages' && authScheme === 'open_ai_bearer_api_key') {
    issues.push({
      field: 'auth_scheme',
      message: 'Anthropic Messages 不能使用 OpenAI Bearer API Key',
    });
  }
  if (baseUrl.trim().length === 0) {
    issues.push({ field: 'base_url', message: '请输入 Base URL' });
  }
  for (const parameter of urlParameters) {
    const value = Object.hasOwn(parameterValues, parameter.name)
      ? (parameterValues[parameter.name] ?? '')
      : '';
    if (value.length === 0) {
      issues.push({
        field: `parameter:${parameter.name}`,
        message: `请输入${parameter.display_name}`,
      });
    } else if (!parameterValueIsValid(parameter.kind, value)) {
      issues.push({
        field: `parameter:${parameter.name}`,
        message:
          parameter.kind === 'domain_label'
            ? `${parameter.display_name}只能含小写字母、数字和中间的连字符，最长 63 个字符`
            : `${parameter.display_name}只能含字母、数字和 - . _ ~，最长 128 个字符`,
      });
    }
  }
  const pathIssue = customModelsPathIssue(discoveryMode, modelsPath);
  if (pathIssue !== undefined) {
    issues.push({ field: 'models_path', message: pathIssue });
  }
  if (apiFormat === 'open_ai_chat_completions') {
    if (chatInstructionRole === '') {
      issues.push({
        field: 'chat_instruction_role',
        message: 'OpenAI Chat Completions 必须选择 Chat Instruction Role',
      });
    }
    if (chatOutputTokenField === '') {
      issues.push({
        field: 'chat_output_token_field',
        message: 'OpenAI Chat Completions 必须选择 Output Token Field',
      });
    }
  }
  for (const [field, value, message] of [
    ['connect_ms', connectMs, 'Connect Timeout 必须是大于 0 的整数'],
    ['first_event_ms', firstEventMs, 'First Event Timeout 必须是大于 0 的整数'],
    ['stream_idle_ms', streamIdleMs, 'Stream Idle Timeout 必须是大于 0 的整数'],
  ] as const) {
    if (!isPositiveInteger(value)) issues.push({ field, message });
  }
  if (totalMs !== '' && !isPositiveInteger(totalMs)) {
    issues.push({ field: 'total_ms', message: 'Total Timeout 必须留空或填写大于 0 的整数' });
  }
  if (!isPositiveInteger(maxParallelRequests)) {
    issues.push({
      field: 'max_parallel_model_requests',
      message: 'Endpoint 并发上限必须是大于 0 的整数',
    });
  }
  return issues;
};
