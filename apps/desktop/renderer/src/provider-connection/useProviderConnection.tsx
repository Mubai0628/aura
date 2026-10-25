import type {
  EndpointAuthScheme,
  EndpointCompatibilitySnapshot,
  IpcErrorEnvelope,
  ModelApiFormat,
  ModelDiscoveryMode,
  ModelEndpointConfig,
  ModelEndpointCreateRequest,
  ModelEndpointUpdateRequest,
  ModelSettingsProjection,
  ProviderPresetVariantView,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import type { SecretApiKeyInputHandle } from '../SecretApiKeyInput';
import type { ProviderConfigurationView } from '../model-settings-types';
import type { NotificationInput } from '../notification-center-store';
import {
  type EndpointEditorField,
  type EndpointEditorValidationIssue,
  endpointEditorValidationIssues,
} from '../provider-connection-validation';
import {
  DEFAULT_TIMEOUTS,
  type DiscoveryModeChoice,
  type EditorTarget,
  isEndpointDraftDirty,
  type PendingDeletion,
  presetMatchesEndpoint,
} from './endpoint-draft';
import { endpointFieldIssueFromIpc } from './endpoint-errors';
import { notifyCredentialFailure, notifyDeletion, notifyFailure } from './notices';

export interface ProviderConnectionDialogProps {
  onClose: () => void;
  onBrowseCatalog: () => void;
  onNotify: (notification: NotificationInput) => void;
  onSaved: () => Promise<void>;
  provider: ProviderConfigurationView;
  settings: ModelSettingsProjection;
  writeUnavailableReason: string | undefined;
}

/**
 * The provider dialog's state, effects and actions. The views read what they show from the returned object.
 */
export function useProviderConnection({
  onClose,
  onBrowseCatalog,
  onNotify,
  onSaved,
  provider,
  settings,
  writeUnavailableReason,
}: ProviderConnectionDialogProps) {
  const [editorTarget, setEditorTarget] = useState<EditorTarget>();
  const [apiFormat, setApiFormat] = useState<ModelApiFormat | ''>('');
  const [authScheme, setAuthScheme] = useState<EndpointAuthScheme>('none');
  const [baseUrl, setBaseUrl] = useState('');
  const [connectionName, setConnectionName] = useState('');
  const [discoveryMode, setDiscoveryMode] = useState<DiscoveryModeChoice>('automatic');
  const [modelsPath, setModelsPath] = useState('');
  const [openAiOrganizationId, setOpenAiOrganizationId] = useState('');
  const [openAiProjectId, setOpenAiProjectId] = useState('');
  const [anthropicWorkspaceId, setAnthropicWorkspaceId] = useState('');
  const [chatInstructionRole, setChatInstructionRole] = useState<'' | 'developer' | 'system'>('');
  const [chatOutputTokenField, setChatOutputTokenField] = useState<
    '' | 'max_completion_tokens' | 'max_tokens'
  >('');
  const [connectMs, setConnectMs] = useState<number>(DEFAULT_TIMEOUTS.connect_ms);
  const [firstEventMs, setFirstEventMs] = useState<number>(DEFAULT_TIMEOUTS.first_event_ms);
  const [streamIdleMs, setStreamIdleMs] = useState<number>(DEFAULT_TIMEOUTS.stream_idle_ms);
  const [totalMs, setTotalMs] = useState<number | ''>('');
  const [maxParallelRequests, setMaxParallelRequests] = useState(2);
  const [selectedPresetVariantId, setSelectedPresetVariantId] = useState('');
  const [baseUrlSuggestionActive, setBaseUrlSuggestionActive] = useState(false);
  const [busy, setBusy] = useState(false);
  const [pendingDeletion, setPendingDeletion] = useState<PendingDeletion>();
  const [showValidationErrors, setShowValidationErrors] = useState(false);
  const [serverFieldErrors, setServerFieldErrors] = useState<
    Partial<Record<EndpointEditorField, string>>
  >({});
  const [pulsingFields, setPulsingFields] = useState<Set<EndpointEditorField>>(() => new Set());
  const [hasSecretDraft, setHasSecretDraft] = useState(false);
  const [providerDisplayNameDraft, setProviderDisplayNameDraft] = useState({
    providerId: provider.id,
    value: provider.localProvider?.display_name ?? provider.name,
  });
  const [compatibilityState, setCompatibilityState] = useState<{
    endpointId: string;
    value: EndpointCompatibilitySnapshot;
  }>();
  const secretInputRef = useRef<SecretApiKeyInputHandle>(null);
  const addConnectionButtonRef = useRef<HTMLButtonElement>(null);
  const deletionTriggerRef = useRef<HTMLElement>(null);
  const focusEndpointSuccessorRef = useRef(false);
  const fieldElementRefs = useRef<
    Partial<Record<EndpointEditorField, HTMLInputElement | HTMLSelectElement>>
  >({});
  const existingEndpoint =
    editorTarget === undefined || editorTarget === 'new'
      ? undefined
      : provider.endpoints.find((endpoint) => endpoint.id === editorTarget);
  const credentialStatus =
    existingEndpoint === undefined
      ? undefined
      : settings.active_credentials.find(
          (credential) =>
            credential.endpoint_id === existingEndpoint.id && credential.slot_name === 'api-key',
        );
  const dormantCredential =
    existingEndpoint === undefined
      ? undefined
      : settings.dormant_credentials.find(
          (credential) =>
            credential.endpoint_id === existingEndpoint.id && credential.slot_name === 'api-key',
        );
  const titleId = `provider-dialog-${provider.id}`;
  const saveIssueId = `${titleId}-save-issue`;
  const providerDisplayName =
    providerDisplayNameDraft.providerId === provider.id
      ? providerDisplayNameDraft.value
      : (provider.localProvider?.display_name ?? provider.name);
  const compatibility =
    existingEndpoint !== undefined && compatibilityState?.endpointId === existingEndpoint.id
      ? compatibilityState.value
      : undefined;
  const existingEndpointId = existingEndpoint?.id;
  const writesDisabled = writeUnavailableReason !== undefined;
  const presetVariants =
    provider.connectionDefaults?.source === 'aura_preset'
      ? provider.connectionDefaults.variants
      : [];
  const modelsDevBaseUrlSuggestion =
    provider.connectionDefaults?.source === 'models_dev_api_suggestion'
      ? provider.connectionDefaults.base_url
      : undefined;

  const setFieldElement =
    (field: EndpointEditorField) =>
    (element: HTMLInputElement | HTMLSelectElement | null): void => {
      if (element === null) {
        delete fieldElementRefs.current[field];
      } else {
        fieldElementRefs.current[field] = element;
      }
    };

  const clearServerFieldError = (field: EndpointEditorField): void => {
    setServerFieldErrors((current) => {
      if (current[field] === undefined) return current;
      const next = { ...current };
      delete next[field];
      return next;
    });
  };

  const resetEditorValidation = (): void => {
    setShowValidationErrors(false);
    setServerFieldErrors({});
    setPulsingFields(new Set());
  };

  const focusField = (field: EndpointEditorField): void => {
    window.requestAnimationFrame(() => fieldElementRefs.current[field]?.focus());
  };

  useEffect(() => {
    if (existingEndpointId === undefined) return;
    let active = true;
    void window.aura.v1
      .getModelCompatibility({ endpoint_id: existingEndpointId })
      .then((result) => {
        if (active && result.ok) {
          setCompatibilityState({ endpointId: existingEndpointId, value: result.value });
        }
      });
    return () => {
      active = false;
    };
  }, [existingEndpointId]);

  useEffect(() => {
    if (editorTarget !== undefined || !focusEndpointSuccessorRef.current) return;
    focusEndpointSuccessorRef.current = false;
    addConnectionButtonRef.current?.focus();
  }, [editorTarget]);

  const close = () => {
    secretInputRef.current?.clear();
    onClose();
  };

  const beginEditing = (endpoint?: ModelEndpointConfig) => {
    secretInputRef.current?.clear();
    setHasSecretDraft(false);
    resetEditorValidation();
    if (endpoint === undefined) {
      const preset = presetVariants.find((variant) => variant.recommended) ?? presetVariants[0];
      setEditorTarget('new');
      if (preset === undefined) {
        setSelectedPresetVariantId('');
        setApiFormat('');
        setAuthScheme('none');
        setBaseUrl(modelsDevBaseUrlSuggestion ?? '');
        setConnectionName('');
        setDiscoveryMode('automatic');
        setModelsPath('');
        setChatInstructionRole('');
        setChatOutputTokenField('');
        setBaseUrlSuggestionActive(modelsDevBaseUrlSuggestion !== undefined);
      } else {
        applyPresetVariant(preset);
        setConnectionName(preset.display_name);
      }
      setOpenAiOrganizationId('');
      setOpenAiProjectId('');
      setAnthropicWorkspaceId('');
      setConnectMs(DEFAULT_TIMEOUTS.connect_ms);
      setFirstEventMs(DEFAULT_TIMEOUTS.first_event_ms);
      setStreamIdleMs(DEFAULT_TIMEOUTS.stream_idle_ms);
      setTotalMs('');
      setMaxParallelRequests(2);
      focusField('display_name');
      return;
    }
    const matchingPreset = presetVariants.find((variant) =>
      presetMatchesEndpoint(variant, endpoint),
    );
    setEditorTarget(endpoint.id);
    setSelectedPresetVariantId(matchingPreset?.variant_id ?? '');
    setBaseUrlSuggestionActive(false);
    setApiFormat(endpoint.api_format);
    setAuthScheme(endpoint.auth_scheme);
    setBaseUrl(endpoint.base_url);
    setConnectionName(endpoint.display_name);
    setDiscoveryMode(endpoint.discovery_mode.type);
    setModelsPath(
      endpoint.discovery_mode.type === 'custom' ? endpoint.discovery_mode.path_and_query : '',
    );
    setOpenAiOrganizationId(endpoint.openai_organization_id ?? '');
    setOpenAiProjectId(endpoint.openai_project_id ?? '');
    setAnthropicWorkspaceId(endpoint.anthropic_workspace_id ?? '');
    setChatInstructionRole(endpoint.chat_instruction_role ?? '');
    setChatOutputTokenField(endpoint.chat_output_token_field ?? '');
    setConnectMs(endpoint.timeouts.connect_ms);
    setFirstEventMs(endpoint.timeouts.first_event_ms);
    setStreamIdleMs(endpoint.timeouts.stream_idle_ms);
    setTotalMs(endpoint.timeouts.total_ms ?? '');
    setMaxParallelRequests(endpoint.max_parallel_model_requests);
    focusField('display_name');
  };

  const applyPresetVariant = (preset: ProviderPresetVariantView) => {
    setSelectedPresetVariantId(preset.variant_id);
    setBaseUrlSuggestionActive(false);
    setApiFormat(preset.api_format);
    setAuthScheme(preset.auth_scheme);
    setBaseUrl(preset.base_url);
    setDiscoveryMode(preset.discovery_mode.type);
    setModelsPath(
      preset.discovery_mode.type === 'custom' ? preset.discovery_mode.path_and_query : '',
    );
    setChatInstructionRole(preset.chat_instruction_role ?? '');
    setChatOutputTokenField(preset.chat_output_token_field ?? '');
  };

  const addCatalogProvider = async () => {
    if (provider.catalogProviderId === undefined || busy || writesDisabled) return;
    setBusy(true);
    const result = await window.aura.v1.createModelProvider({
      expected_revision: settings.models_revision,
      input: {
        display_name: provider.name,
        source: { catalog_provider_id: provider.catalogProviderId, type: 'models_dev' },
      },
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, 'Provider 未添加', 'model-provider-add', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-provider-added:${result.value.value.id}`,
      message: `${provider.name} 已保存，现在可以添加 Endpoint。`,
      title: 'Provider 已添加',
      tone: 'success',
    });
  };

  const renameProvider = async () => {
    if (
      provider.localProvider === undefined ||
      providerDisplayName.trim().length === 0 ||
      providerDisplayName === provider.localProvider.display_name ||
      busy ||
      writesDisabled
    ) {
      return;
    }
    setBusy(true);
    const result = await window.aura.v1.renameModelProvider({
      display_name: providerDisplayName,
      expected_revision: settings.models_revision,
      provider_id: provider.localProvider.id,
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, 'Provider 未重命名', 'model-provider-rename', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-provider-renamed:${result.value.value.id}:${result.value.revision}`,
      message: `Provider 已重命名为 ${result.value.value.display_name}。`,
      title: 'Provider 已更新',
      tone: 'success',
    });
  };

  /** The provider's default connection and its backups, in order. */
  const setConnections = async (defaultEndpoint: string | null, backups: string[]) => {
    if (provider.localProvider === undefined || busy || writesDisabled) return;
    setBusy(true);
    const result = await window.aura.v1.setModelProviderConnections({
      expected_revision: settings.models_revision,
      provider_id: provider.localProvider.id,
      default_endpoint_id: defaultEndpoint,
      backup_endpoint_ids: backups,
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, '连接设置未保存', 'model-provider-connections', result.error);
      return;
    }
    await onSaved();
  };

  const deleteProvider = async () => {
    if (provider.localProvider === undefined || busy || writesDisabled) {
      return;
    }
    setBusy(true);
    const result = await window.aura.v1.deleteModelProvider({
      expected_revision: settings.models_revision,
      provider_id: provider.localProvider.id,
    });
    setBusy(false);
    if (!result.ok) {
      setPendingDeletion(undefined);
      notifyFailure(onNotify, 'Provider 未删除', 'model-provider-delete', result.error);
      return;
    }
    await onSaved();
    notifyDeletion(
      onNotify,
      provider.localProvider.display_name,
      result.value.value.credential_cleanup_pending,
    );
    setPendingDeletion(undefined);
    close();
  };

  const deleteEndpoint = async () => {
    if (existingEndpoint === undefined || busy || writesDisabled) {
      return;
    }
    setBusy(true);
    const result = await window.aura.v1.deleteModelEndpoint({
      endpoint_id: existingEndpoint.id,
      expected_revision: settings.models_revision,
    });
    setBusy(false);
    if (!result.ok) {
      setPendingDeletion(undefined);
      notifyFailure(onNotify, 'Endpoint 未删除', 'model-endpoint-delete', result.error);
      return;
    }
    await onSaved();
    notifyDeletion(
      onNotify,
      existingEndpoint.display_name,
      result.value.value.credential_cleanup_pending,
    );
    focusEndpointSuccessorRef.current = true;
    setPendingDeletion(undefined);
    setEditorTarget(undefined);
  };

  const resetCompatibility = async () => {
    if (existingEndpoint === undefined || busy || writesDisabled) return;
    setBusy(true);
    const result = await window.aura.v1.resetModelCompatibility({
      endpoint_id: existingEndpoint.id,
      expected_revision: settings.models_revision,
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, 'Compatibility 未重置', 'model-compatibility-reset', result.error);
      return;
    }
    setCompatibilityState({
      endpointId: existingEndpoint.id,
      value: {
        compatibility: result.value.compatibility ?? null,
        endpoint_id: result.value.endpoint_id,
        revision: result.value.revision,
      },
    });
    if (result.value.changed) await onSaved();
    onNotify({
      dedupeKey: `model-compatibility-reset:${existingEndpoint.id}:${result.value.revision}`,
      message: result.value.changed
        ? '当前 Endpoint Fingerprint 的兼容学习已原子清除。'
        : '当前没有兼容学习记录。',
      title: 'Compatibility Reset',
      tone: result.value.changed ? 'success' : 'info',
    });
  };

  const discovery: ModelDiscoveryMode =
    discoveryMode === 'custom'
      ? { path_and_query: modelsPath, type: 'custom' }
      : { type: discoveryMode };
  const editorValidationIssues = endpointEditorValidationIssues({
    apiFormat,
    authScheme,
    baseUrl,
    chatInstructionRole,
    chatOutputTokenField,
    connectMs,
    connectionName,
    discoveryMode,
    firstEventMs,
    hasLocalProvider: provider.localProvider !== undefined,
    maxParallelRequests,
    modelsPath,
    streamIdleMs,
    totalMs,
  });
  const localIssueByField = new Map(
    editorValidationIssues.map((issue) => [issue.field, issue.message] as const),
  );
  const fieldIssue = (field: EndpointEditorField): string | undefined =>
    serverFieldErrors[field] ?? (showValidationErrors ? localIssueByField.get(field) : undefined);
  const fieldIssueId = (field: EndpointEditorField): string => `${titleId}-${field}-issue`;
  const finishFieldPulse = (field: EndpointEditorField): void => {
    setPulsingFields((current) => {
      if (!current.has(field)) return current;
      const next = new Set(current);
      next.delete(field);
      return next;
    });
  };
  const renderFieldIssue = (field: EndpointEditorField) => {
    const issue = fieldIssue(field);
    return issue === undefined ? null : (
      <small className="field-validation-error" id={fieldIssueId(field)} role="alert">
        {issue}
      </small>
    );
  };
  const controlValidationProps = (field: EndpointEditorField) => {
    const issue = fieldIssue(field);
    return {
      'aria-describedby': issue === undefined ? undefined : fieldIssueId(field),
      'aria-invalid': issue !== undefined,
      className: pulsingFields.has(field) ? 'field-validation-pulse' : undefined,
      onAnimationEnd: () => finishFieldPulse(field),
      ref: setFieldElement(field),
    };
  };

  const showIssues = (issues: EndpointEditorValidationIssue[]): void => {
    setShowValidationErrors(true);
    setPulsingFields(new Set(issues.map((issue) => issue.field)));
    const firstField = issues.find((issue) => issue.field !== 'form')?.field;
    if (firstField !== undefined) focusField(firstField);
  };

  const showCoreFieldError = (error: IpcErrorEnvelope): boolean => {
    const issue = endpointFieldIssueFromIpc(error);
    if (issue === undefined) return false;
    setServerFieldErrors({ [issue.field]: issue.message });
    showIssues([issue]);
    return true;
  };

  const commonEndpointFields =
    apiFormat === ''
      ? undefined
      : {
          anthropic_workspace_id:
            apiFormat === 'anthropic_messages' && anthropicWorkspaceId !== ''
              ? anthropicWorkspaceId
              : null,
          api_format: apiFormat,
          auth_scheme: authScheme,
          base_url: baseUrl,
          chat_instruction_role:
            apiFormat === 'open_ai_chat_completions' && chatInstructionRole !== ''
              ? chatInstructionRole
              : null,
          chat_output_token_field:
            apiFormat === 'open_ai_chat_completions' && chatOutputTokenField !== ''
              ? chatOutputTokenField
              : null,
          discovery_mode: discovery,
          display_name: connectionName,
          max_parallel_model_requests: maxParallelRequests,
          openai_organization_id:
            apiFormat !== 'anthropic_messages' && openAiOrganizationId !== ''
              ? openAiOrganizationId
              : null,
          openai_project_id:
            apiFormat !== 'anthropic_messages' && openAiProjectId !== '' ? openAiProjectId : null,
          timeouts: {
            connect_ms: connectMs,
            first_event_ms: firstEventMs,
            stream_idle_ms: streamIdleMs,
            total_ms: totalMs === '' ? null : totalMs,
          },
        };
  const dirty = isEndpointDraftDirty(existingEndpoint, commonEndpointFields);
  const saveDisabledReason =
    writeUnavailableReason ??
    (existingEndpoint !== undefined && !dirty ? '连接配置没有变化' : undefined);

  const saveEndpoint = async () => {
    if (busy || provider.localProvider === undefined || writesDisabled) {
      return;
    }
    setServerFieldErrors({});
    if (editorValidationIssues.length > 0 || commonEndpointFields === undefined) {
      showIssues(editorValidationIssues);
      return;
    }
    setBusy(true);
    if (existingEndpoint === undefined) {
      const request: ModelEndpointCreateRequest = {
        ...commonEndpointFields,
        expected_revision: settings.models_revision,
        provider_id: provider.localProvider.id,
      };
      const secretBytes = secretInputRef.current?.takeBytes();
      if (secretBytes === undefined) {
        const result = await window.aura.v1.createModelEndpoint(request);
        setBusy(false);
        if (!result.ok) {
          if (showCoreFieldError(result.error)) return;
          notifyFailure(onNotify, 'Endpoint 未保存', 'model-endpoint-save', result.error);
          return;
        }
        await finishEndpointSave(result.value.value.display_name, result.value.value.id);
      } else {
        let result;
        try {
          result = await window.aura.v1.createModelEndpointWithCredential(request, secretBytes);
        } finally {
          secretBytes.fill(0);
        }
        setBusy(false);
        if (!result.ok) {
          if (showCoreFieldError(result.error)) return;
          notifyFailure(onNotify, 'Endpoint 未保存', 'model-endpoint-save', result.error);
          return;
        }
        await finishEndpointSave(
          result.value.endpoint_projection.endpoint.display_name,
          result.value.endpoint_projection.endpoint.id,
        );
      }
      return;
    } else {
      secretInputRef.current?.clear();
      const request: ModelEndpointUpdateRequest = {
        ...commonEndpointFields,
        endpoint_id: existingEndpoint.id,
        expected_endpoint_revision: existingEndpoint.revision,
        expected_models_revision: settings.models_revision,
      };
      const result = await window.aura.v1.updateModelEndpoint(request);
      setBusy(false);
      if (!result.ok) {
        if (showCoreFieldError(result.error)) return;
        if (result.error.code === 'models.configuration.revision_conflict') {
          await onSaved();
          onNotify({
            dedupeKey: `model-endpoint-save-conflict:${existingEndpoint.id}`,
            message: '配置已在其他操作中更新。已刷新最新版本并保留当前表单草稿，请核对后再次保存。',
            title: 'Endpoint 配置已变化',
            tone: 'warning',
          });
          return;
        }
        notifyFailure(onNotify, 'Endpoint 未保存', 'model-endpoint-save', result.error);
        return;
      }
      await finishEndpointSave(result.value.value.display_name, result.value.value.id);
    }
  };

  const finishEndpointSave = async (displayName: string, endpointKey: string) => {
    await onSaved();
    onNotify({
      dedupeKey: `model-endpoint-saved:${endpointKey}`,
      message: `${displayName} 已写入 Core 模型配置。`,
      title: 'Endpoint 已保存',
      tone: 'success',
    });
    setEditorTarget(undefined);
  };

  const saveCredential = async () => {
    if (
      existingEndpoint === undefined ||
      credentialStatus === undefined ||
      busy ||
      writesDisabled
    ) {
      return;
    }
    // The command follows the credential state machine, not the presence of an
    // earlier committed generation: after a clear there is no active secret, so
    // the slot is configured again (which still advances the generation).
    const writable =
      credentialStatus.state === 'configured'
        ? 'replace'
        : credentialStatus.state === 'unconfigured' || credentialStatus.state === 'cleared'
          ? 'configure'
          : undefined;
    if (writable === undefined) {
      notifyCredentialFailure(onNotify, 'API Key 未保存', 'model-credential-save', {
        code: 'credential.state_conflict',
        message: '',
      });
      return;
    }
    const secretBytes = secretInputRef.current?.takeBytes();
    if (secretBytes === undefined) return;
    setBusy(true);
    const request = {
      credential_id: credentialStatus.credential_id,
      credential_slot: 'api-key' as const,
      endpoint_id: existingEndpoint.id,
      expected_models_revision: settings.models_revision,
      expected_row_revision: credentialStatus.row_revision,
    };
    let result;
    try {
      const operation =
        writable === 'replace'
          ? window.aura.v1.replaceModelCredential(request, secretBytes)
          : window.aura.v1.configureModelCredential(request, secretBytes);
      secretBytes.fill(0);
      result = await operation;
    } finally {
      secretBytes.fill(0);
    }
    setBusy(false);
    if (!result.ok) {
      notifyCredentialFailure(onNotify, 'API Key 未保存', 'model-credential-save', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-credential-saved:${result.value.endpoint_id}:${result.value.credential_status.row_revision}`,
      message: 'API Key 已通过专用 SecretInput 通道写入 Core 凭据槽位。',
      title: 'API Key 已保存',
      tone: 'success',
    });
  };

  const clearCredential = async () => {
    const credential = credentialStatus ?? dormantCredential;
    if (existingEndpoint === undefined || credential === undefined || busy || writesDisabled)
      return;
    setBusy(true);
    const result = await window.aura.v1.clearModelCredential({
      credential_id: credential.credential_id,
      credential_slot: 'api-key',
      endpoint_id: existingEndpoint.id,
      expected_models_revision: settings.models_revision,
      expected_row_revision: credential.row_revision,
    });
    setBusy(false);
    if (!result.ok) {
      notifyCredentialFailure(onNotify, 'API Key 未清除', 'model-credential-clear', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `model-credential-cleared:${result.value.endpoint_id}:${result.value.credential_status.row_revision}`,
      message: '凭据已清除；CredentialId 与最后提交代次按冻结 Contract 保留。',
      title: 'API Key 已清除',
      tone: 'success',
    });
  };

  const discoverModels = async () => {
    if (existingEndpoint === undefined || dirty || busy || writesDisabled) return;
    setBusy(true);
    const result = await window.aura.v1.startModelDiscovery({
      endpoint_id: existingEndpoint.id,
      expected_endpoint_revision: existingEndpoint.revision,
      expected_models_revision: settings.models_revision,
    });
    setBusy(false);
    if (!result.ok) {
      notifyFailure(onNotify, '模型目录请求未完成', 'model-discovery', result.error);
      return;
    }
    onNotify({
      dedupeKey: `model-discovery:${result.value.model_discovery_run_id}`,
      message: '模型目录请求已进入后台队列，可关闭此页面；操作不会因退订而取消。',
      title: '模型目录请求已开始',
      tone: 'success',
    });
  };

  return {
    onClose,
    onBrowseCatalog,
    onNotify,
    onSaved,
    provider,
    settings,
    writeUnavailableReason,
    editorTarget,
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
    setBusy,
    pendingDeletion,
    setPendingDeletion,
    showValidationErrors,
    setShowValidationErrors,
    serverFieldErrors,
    setServerFieldErrors,
    pulsingFields,
    setPulsingFields,
    hasSecretDraft,
    setHasSecretDraft,
    providerDisplayNameDraft,
    setProviderDisplayNameDraft,
    compatibilityState,
    setCompatibilityState,
    secretInputRef,
    addConnectionButtonRef,
    deletionTriggerRef,
    focusEndpointSuccessorRef,
    fieldElementRefs,
    existingEndpoint,
    credentialStatus,
    dormantCredential,
    titleId,
    saveIssueId,
    providerDisplayName,
    compatibility,
    existingEndpointId,
    writesDisabled,
    presetVariants,
    modelsDevBaseUrlSuggestion,
    setFieldElement,
    clearServerFieldError,
    resetEditorValidation,
    focusField,
    close,
    beginEditing,
    applyPresetVariant,
    addCatalogProvider,
    renameProvider,
    setConnections,
    deleteProvider,
    deleteEndpoint,
    resetCompatibility,
    discovery,
    editorValidationIssues,
    localIssueByField,
    fieldIssue,
    fieldIssueId,
    finishFieldPulse,
    renderFieldIssue,
    controlValidationProps,
    showIssues,
    showCoreFieldError,
    commonEndpointFields,
    dirty,
    saveDisabledReason,
    saveEndpoint,
    finishEndpointSave,
    saveCredential,
    clearCredential,
    discoverModels,
  };
}

export type ProviderConnection = ReturnType<typeof useProviderConnection>;
