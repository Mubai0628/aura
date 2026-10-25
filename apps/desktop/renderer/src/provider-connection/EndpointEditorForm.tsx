import type { EndpointAuthScheme, ModelApiFormat } from '@aura/contracts/generated';
import * as Collapsible from '@radix-ui/react-collapsible';
import { useState } from 'react';
import { SecretApiKeyInput } from '../SecretApiKeyInput';
import { AppIcon } from '../app-icons';
import { ENDPOINT_API_TYPES } from '../model-settings-types';
import { EndpointOperationHistory } from './EndpointOperationHistory';
import { credentialStatusTitle } from './credential-status';
import type { EndpointEditorField } from '../provider-connection-validation';
import type { DiscoveryModeChoice } from './endpoint-draft';
import type { ProviderConnection } from './useProviderConnection';

/** Fields with defaults or that most connections leave empty; they open by themselves when
 *  one of them needs attention. */
const ADVANCED_FIELDS: readonly EndpointEditorField[] = [
  'connect_ms',
  'first_event_ms',
  'stream_idle_ms',
  'total_ms',
  'max_parallel_model_requests',
];

/** One connection's editor: preset, address, API type, credential, discovery and limits. */
export function EndpointEditorForm({ connection }: { connection: ProviderConnection }) {
  const {
    onNotify,
    writeUnavailableReason,
    setEditorTarget,
    apiFormat,
    setApiFormat,
    authScheme,
    setAuthScheme,
    baseUrl,
    setBaseUrl,
    connectionName,
    setConnectionName,
    discoveryMode,
    setDiscoveryMode,
    modelsPath,
    setModelsPath,
    openAiOrganizationId,
    setOpenAiOrganizationId,
    openAiProjectId,
    setOpenAiProjectId,
    anthropicWorkspaceId,
    setAnthropicWorkspaceId,
    chatInstructionRole,
    setChatInstructionRole,
    chatOutputTokenField,
    setChatOutputTokenField,
    connectMs,
    setConnectMs,
    firstEventMs,
    setFirstEventMs,
    streamIdleMs,
    setStreamIdleMs,
    totalMs,
    setTotalMs,
    maxParallelRequests,
    setMaxParallelRequests,
    selectedPresetVariantId,
    setSelectedPresetVariantId,
    baseUrlSuggestionActive,
    setBaseUrlSuggestionActive,
    busy,
    setPendingDeletion,
    hasSecretDraft,
    setHasSecretDraft,
    secretInputRef,
    deletionTriggerRef,
    existingEndpoint,
    credentialStatus,
    dormantCredential,
    saveIssueId,
    compatibility,
    writesDisabled,
    presetVariants,
    clearServerFieldError,
    applyPresetVariant,
    resetCompatibility,
    fieldIssue,
    renderFieldIssue,
    dirty,
    saveDisabledReason,
    saveEndpoint,
    saveCredential,
    clearCredential,
    discoverModels,
    controlValidationProps,
  } = connection;
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const advancedIssue = ADVANCED_FIELDS.some((field) => fieldIssue(field) !== undefined);
  return (
    <form
      className="provider-editor"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        void saveEndpoint();
      }}
    >
      {existingEndpoint === undefined ? null : (
        <EndpointOperationHistory endpointId={existingEndpoint.id} onNotify={onNotify} />
      )}
      <div className="settings-form-grid provider-form-grid">
        {presetVariants.length === 0 ? null : (
          <label className="span-two">
            <span>Aura 连接预设</span>
            <select
              onChange={(event) => {
                const preset = presetVariants.find(
                  (variant) => variant.variant_id === event.target.value,
                );
                if (preset === undefined) {
                  setSelectedPresetVariantId('');
                  return;
                }
                applyPresetVariant(preset);
                setConnectionName(preset.display_name);
              }}
              value={selectedPresetVariantId}
            >
              <option value="">自定义执行配置</option>
              {presetVariants.map((variant) => (
                <option key={variant.variant_id} value={variant.variant_id}>
                  {variant.display_name}
                  {variant.recommended ? '（推荐）' : ''}
                </option>
              ))}
            </select>
          </label>
        )}
        <label>
          <span>连接名称</span>
          <input
            {...controlValidationProps('display_name')}
            aria-label="连接名称"
            onChange={(event) => {
              clearServerFieldError('display_name');
              setConnectionName(event.target.value);
            }}
            type="text"
            value={connectionName}
          />
          {renderFieldIssue('display_name')}
        </label>
        <label>
          <span>API 类型</span>
          <select
            {...controlValidationProps('api_format')}
            aria-label="API 类型"
            onChange={(event) => {
              clearServerFieldError('api_format');
              setSelectedPresetVariantId('');
              setApiFormat(event.target.value as ModelApiFormat | '');
            }}
            value={apiFormat}
          >
            <option value="">请选择</option>
            {ENDPOINT_API_TYPES.map((type) => (
              <option key={type.value} value={type.value}>
                {type.label}
              </option>
            ))}
          </select>
          {renderFieldIssue('api_format')}
        </label>
        <label>
          <span>认证方式</span>
          <select
            {...controlValidationProps('auth_scheme')}
            aria-label="认证方式"
            onChange={(event) => {
              clearServerFieldError('auth_scheme');
              setSelectedPresetVariantId('');
              const next = event.target.value as EndpointAuthScheme;
              setAuthScheme(next);
              if (next === 'none') secretInputRef.current?.clear();
            }}
            value={authScheme}
          >
            <option value="none">None</option>
            <option value="open_ai_bearer_api_key">OpenAI Bearer API Key</option>
            <option value="anthropic_api_key">Anthropic API Key</option>
          </select>
          {renderFieldIssue('auth_scheme')}
        </label>
        <label className="span-two">
          <span>Base URL</span>
          <input
            {...controlValidationProps('base_url')}
            aria-label="Base URL"
            onChange={(event) => {
              clearServerFieldError('base_url');
              setSelectedPresetVariantId('');
              setBaseUrlSuggestionActive(false);
              setBaseUrl(event.target.value);
            }}
            placeholder="例如 https://api.example.com/v1"
            type="url"
            value={baseUrl}
          />
          {renderFieldIssue('base_url')}
          {baseUrlSuggestionActive ? (
            <small className="catalog-field-source">来自 models.dev 的未保存建议</small>
          ) : null}
        </label>
        <label>
          <span>模型目录请求模式</span>
          <select
            onChange={(event) => {
              setSelectedPresetVariantId('');
              setDiscoveryMode(event.target.value as DiscoveryModeChoice);
            }}
            value={discoveryMode}
          >
            <option value="automatic">Automatic</option>
            <option value="custom">Custom</option>
            <option value="disabled">Disabled</option>
          </select>
        </label>
        <label>
          <span>模型目录请求路径</span>
          <input
            {...controlValidationProps('models_path')}
            aria-label="模型目录请求路径"
            disabled={discoveryMode !== 'custom'}
            onChange={(event) => {
              clearServerFieldError('models_path');
              setSelectedPresetVariantId('');
              setModelsPath(event.target.value);
            }}
            placeholder="例如 /v1/models"
            type="text"
            value={modelsPath}
          />
          {renderFieldIssue('models_path')}
        </label>
        {apiFormat === 'open_ai_chat_completions' ? (
          <>
            <label>
              <span>Chat Instruction Role</span>
              <select
                {...controlValidationProps('chat_instruction_role')}
                aria-label="Chat Instruction Role"
                aria-required="true"
                onChange={(event) => {
                  clearServerFieldError('chat_instruction_role');
                  setSelectedPresetVariantId('');
                  setChatInstructionRole(event.target.value as typeof chatInstructionRole);
                }}
                value={chatInstructionRole}
              >
                <option value="">未配置</option>
                <option value="developer">Developer</option>
                <option value="system">System</option>
              </select>
              {renderFieldIssue('chat_instruction_role')}
            </label>
            <label>
              <span>Output Token Field</span>
              <select
                {...controlValidationProps('chat_output_token_field')}
                aria-label="Output Token Field"
                aria-required="true"
                onChange={(event) => {
                  clearServerFieldError('chat_output_token_field');
                  setSelectedPresetVariantId('');
                  setChatOutputTokenField(event.target.value as typeof chatOutputTokenField);
                }}
                value={chatOutputTokenField}
              >
                <option value="">未配置</option>
                <option value="max_completion_tokens">max_completion_tokens</option>
                <option value="max_tokens">max_tokens</option>
              </select>
              {renderFieldIssue('chat_output_token_field')}
            </label>
          </>
        ) : null}
        <Collapsible.Root
          className="profile-context-tools endpoint-advanced span-two"
          onOpenChange={setAdvancedOpen}
          open={advancedOpen || advancedIssue}
        >
          <Collapsible.Trigger className="context-tools-trigger" type="button">
            <span>高级设置</span>
            <AppIcon name="chevron-down" size={14} />
          </Collapsible.Trigger>
          <Collapsible.Content className="settings-form-grid provider-form-grid">
            {apiFormat === 'anthropic_messages' ? (
              <label className="span-two">
                <span>Anthropic Workspace ID（可选）</span>
                <input
                  onChange={(event) => setAnthropicWorkspaceId(event.target.value)}
                  type="text"
                  value={anthropicWorkspaceId}
                />
              </label>
            ) : (
              <>
                <label>
                  <span>OpenAI Organization ID（可选）</span>
                  <input
                    onChange={(event) => setOpenAiOrganizationId(event.target.value)}
                    type="text"
                    value={openAiOrganizationId}
                  />
                </label>
                <label>
                  <span>OpenAI Project ID（可选）</span>
                  <input
                    onChange={(event) => setOpenAiProjectId(event.target.value)}
                    type="text"
                    value={openAiProjectId}
                  />
                </label>
              </>
            )}
            <label>
              <span>Connect Timeout（ms）</span>
              <input
                {...controlValidationProps('connect_ms')}
                aria-label="Connect Timeout（ms）"
                min="1"
                onChange={(event) => {
                  clearServerFieldError('connect_ms');
                  setConnectMs(event.target.valueAsNumber);
                }}
                type="number"
                value={connectMs}
              />
              {renderFieldIssue('connect_ms')}
            </label>
            <label>
              <span>First Event Timeout（ms）</span>
              <input
                {...controlValidationProps('first_event_ms')}
                aria-label="First Event Timeout（ms）"
                min="1"
                onChange={(event) => {
                  clearServerFieldError('first_event_ms');
                  setFirstEventMs(event.target.valueAsNumber);
                }}
                type="number"
                value={firstEventMs}
              />
              {renderFieldIssue('first_event_ms')}
            </label>
            <label>
              <span>Stream Idle Timeout（ms）</span>
              <input
                {...controlValidationProps('stream_idle_ms')}
                aria-label="Stream Idle Timeout（ms）"
                min="1"
                onChange={(event) => {
                  clearServerFieldError('stream_idle_ms');
                  setStreamIdleMs(event.target.valueAsNumber);
                }}
                type="number"
                value={streamIdleMs}
              />
              {renderFieldIssue('stream_idle_ms')}
            </label>
            <label>
              <span>Total Timeout（ms，可选）</span>
              <input
                {...controlValidationProps('total_ms')}
                aria-label="Total Timeout（ms，可选）"
                min="1"
                onChange={(event) => {
                  clearServerFieldError('total_ms');
                  setTotalMs(event.target.value === '' ? '' : event.target.valueAsNumber);
                }}
                type="number"
                value={totalMs}
              />
              {renderFieldIssue('total_ms')}
            </label>
            <label>
              <span>Endpoint 并发上限</span>
              <input
                {...controlValidationProps('max_parallel_model_requests')}
                aria-label="Endpoint 并发上限"
                min="1"
                onChange={(event) => {
                  clearServerFieldError('max_parallel_model_requests');
                  setMaxParallelRequests(event.target.valueAsNumber);
                }}
                type="number"
                value={maxParallelRequests}
              />
              {renderFieldIssue('max_parallel_model_requests')}
            </label>
          </Collapsible.Content>
        </Collapsible.Root>
      </div>

      {authScheme === 'none' ? (
        dormantCredential === undefined ? null : (
          <div className="credential-boundary credential-input-boundary">
            <div className="credential-copy">
              <span className="credential-icon">
                <AppIcon name="shield" size={16} />
              </span>
              <div>
                <strong>已保留凭据，当前未使用</strong>
                <p>Endpoint 当前无需认证；已保留的凭据不会被发送，可由你明确清除。</p>
              </div>
            </div>
            <div className="credential-actions">
              <button
                disabled={
                  busy ||
                  writesDisabled ||
                  dormantCredential.retained_state === 'reconciliation_required'
                }
                onClick={() => void clearCredential()}
                type="button"
              >
                清除已保留凭据
              </button>
            </div>
          </div>
        )
      ) : (
        <div className="credential-boundary credential-input-boundary">
          <div className="credential-input-copy">
            <div className="credential-copy">
              <span className="credential-icon">
                <AppIcon name="shield" size={16} />
              </span>
              <div>
                <strong>{credentialStatusTitle(credentialStatus)}</strong>
              </div>
            </div>
            <SecretApiKeyInput
              disabled={
                busy || writesDisabled || credentialStatus?.state === 'reconciliation_required'
              }
              onDraftStateChange={setHasSecretDraft}
              ref={secretInputRef}
              status={credentialStatus}
            />
          </div>
          {existingEndpoint === undefined ? null : (
            <div className="credential-actions">
              <button
                disabled={
                  !hasSecretDraft ||
                  busy ||
                  writesDisabled ||
                  credentialStatus?.state === 'reconciliation_required'
                }
                onClick={() => void saveCredential()}
                type="button"
              >
                保存 API Key
              </button>
              <button
                disabled={
                  busy ||
                  writesDisabled ||
                  credentialStatus === undefined ||
                  credentialStatus.state === 'unconfigured' ||
                  credentialStatus.state === 'cleared'
                }
                onClick={() => void clearCredential()}
                type="button"
              >
                清除
              </button>
            </div>
          )}
        </div>
      )}

      {existingEndpoint === undefined ? null : (
        <section className="compatibility-panel" aria-label="Compatibility Learning">
          <div className="compatibility-summary">
            <strong>Compatibility Learning</strong>
            <p>
              {compatibility?.compatibility === null || compatibility?.compatibility === undefined
                ? '当前 Endpoint Fingerprint 没有兼容学习记录。'
                : `${compatibility.compatibility.output_token_field_aliases.length} 条输出字段别名；${compatibility.compatibility.unsupported_features.length} 条特性记录。`}
            </p>
            {compatibility?.compatibility === null ||
            compatibility?.compatibility === undefined ? null : (
              <div className="compatibility-details">
                {compatibility.compatibility.output_token_field_aliases.length === 0 ? null : (
                  <div>
                    <span>输出 Token 字段别名</span>
                    <ul>
                      {compatibility.compatibility.output_token_field_aliases.map((alias) => (
                        <li key={alias.provider_model_name}>
                          <code>{alias.provider_model_name}</code>：
                          <code>{alias.configured_field}</code> →{' '}
                          <code>{alias.effective_field}</code>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                {compatibility.compatibility.unsupported_features.length === 0 ? null : (
                  <div>
                    <span>不支持的特性</span>
                    <ul>
                      {compatibility.compatibility.unsupported_features.map((feature) => (
                        <li key={feature}>
                          <code>{feature}</code>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
                {compatibility.compatibility.unsupported_parameters.length === 0 ? null : (
                  <div>
                    <span>不支持的参数</span>
                    <ul>
                      {compatibility.compatibility.unsupported_parameters.map((parameter) => (
                        <li key={parameter}>
                          <code>{parameter}</code>
                        </li>
                      ))}
                    </ul>
                  </div>
                )}
              </div>
            )}
          </div>
          <button
            disabled={busy || writesDisabled}
            onClick={() => void resetCompatibility()}
            type="button"
          >
            Reset
          </button>
        </section>
      )}

      <footer className="modal-actions">
        <button onClick={() => setEditorTarget(undefined)} type="button">
          返回连接列表
        </button>
        {existingEndpoint === undefined ? null : (
          <button
            className="danger-button"
            disabled={busy || writesDisabled}
            onClick={(event) => {
              deletionTriggerRef.current = event.currentTarget;
              setPendingDeletion({
                displayName: existingEndpoint.display_name,
                kind: 'endpoint',
              });
            }}
            type="button"
          >
            删除 Endpoint
          </button>
        )}
        <span className="modal-action-spacer" />
        {existingEndpoint === undefined ? null : (
          <span className="disabled-action-wrapper" tabIndex={dirty || writesDisabled ? 0 : -1}>
            <button
              disabled={dirty || busy || writesDisabled || discoveryMode === 'disabled'}
              onClick={() => void discoverModels()}
              type="button"
            >
              获取模型
            </button>
            {dirty || writesDisabled ? (
              <span role="tooltip">{writeUnavailableReason ?? '先保存连接后再获取模型'}</span>
            ) : null}
          </span>
        )}
        <span
          aria-describedby={!busy && saveDisabledReason !== undefined ? saveIssueId : undefined}
          className="disabled-action-wrapper"
          tabIndex={!busy && saveDisabledReason !== undefined ? 0 : -1}
        >
          <button
            className="primary-button"
            disabled={busy || writesDisabled || (existingEndpoint !== undefined && !dirty)}
            type="submit"
          >
            {busy ? '保存中…' : '保存连接'}
          </button>
          {!busy && saveDisabledReason !== undefined ? (
            <span id={saveIssueId} role="tooltip">
              {saveDisabledReason}
            </span>
          ) : null}
        </span>
      </footer>
    </form>
  );
}
