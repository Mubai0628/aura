import { describe, expect, it } from 'vitest';

import {
  customModelsPathIssue,
  endpointEditorValidationIssues,
} from './provider-connection-validation';

const validEditor = {
  apiFormat: 'open_ai_chat_completions' as const,
  authScheme: 'open_ai_bearer_api_key' as const,
  baseUrl: 'https://api.example.com/v1',
  chatInstructionRole: 'system' as const,
  chatOutputTokenField: 'max_completion_tokens' as const,
  connectMs: 15_000,
  connectionName: 'Primary',
  discoveryMode: 'custom' as const,
  firstEventMs: 120_000,
  hasLocalProvider: true,
  maxParallelRequests: 2,
  modelsPath: '/models',
  streamIdleMs: 120_000,
  totalMs: '' as const,
};

describe('provider connection validation', () => {
  it('returns field-addressable issues for all correctable invalid values', () => {
    const issues = endpointEditorValidationIssues({
      ...validEditor,
      chatInstructionRole: '' as const,
      chatOutputTokenField: '' as const,
      connectMs: Number.NaN,
      modelsPath: 'models',
    });

    expect(issues.map((issue) => issue.field)).toEqual([
      'models_path',
      'chat_instruction_role',
      'chat_output_token_field',
      'connect_ms',
    ]);
    expect(customModelsPathIssue('custom', 'models')).toBe(
      '模型目录请求路径必须以 / 开头，例如 /models',
    );
  });

  it('requires both OpenAI Chat fields and accepts a complete Chat configuration', () => {
    expect(endpointEditorValidationIssues(validEditor)).toEqual([]);
    expect(
      endpointEditorValidationIssues({
        ...validEditor,
        chatOutputTokenField: '' as const,
      }),
    ).toContainEqual({
      field: 'chat_output_token_field',
      message: 'OpenAI Chat Completions 必须选择 Output Token Field',
    });
  });

  it('ignores a model path outside custom discovery and validates API/auth pairing', () => {
    expect(
      endpointEditorValidationIssues({
        ...validEditor,
        discoveryMode: 'automatic',
        modelsPath: '',
      }),
    ).toEqual([]);
    expect(
      endpointEditorValidationIssues({
        ...validEditor,
        authScheme: 'anthropic_api_key',
      }).map((issue) => issue.field),
    ).toContain('auth_scheme');
  });
});
