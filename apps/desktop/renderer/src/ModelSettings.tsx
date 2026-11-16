import type {
  ModelProfileConfig,
  ModelSettingsProjection,
  ProviderCatalogProjection,
} from '@aura/contracts/generated';
import { useEffect, useMemo, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import { userMessage } from './agent-session/user-message';
import { CatalogModelDialog } from './CatalogModelDialog';
import { ConfirmationDialog } from './ConfirmationDialog';
import { CustomProviderDialog } from './CustomProviderDialog';
import { ModelProfileCard } from './ModelProfileCard';
import { ModelSectionToolbar } from './ModelSectionToolbar';
import { isProfileReady, profileMatches } from './model-profile-status';
import { ModelProfileDialog } from './ModelProfileDialog';
import { NetworkProxySection } from './network-proxy/NetworkProxySection';
import type { ProviderCatalogItem, ProviderConfigurationView } from './model-settings-types';
import { providerInitials } from './model-settings-types';
import type { NotificationInput } from './notification-center-store';
import { useModelSettingsControl } from './model-settings-store';
import { ProviderCatalog } from './ProviderCatalog';
import { ProviderConnectionDialog } from './ProviderConnectionDialog';
import { useSettingsDisplayPreferences } from './settings-display-preferences';
import {
  CATALOG_ENTRY_MISSING,
  CATALOG_ENTRY_UPDATED,
  catalogUnavailableText,
  modelFailureText,
} from './provider-connection/model-failure-words';

interface ModelSettingsProps {
  onNotify: (notification: NotificationInput) => void;
  /** Withdraws this page's "model settings unavailable" notice once they are available again. */
  onWithdrawNotice?: (prefix: string) => void;
}

export function ModelSettings({ onNotify, onWithdrawNotice }: ModelSettingsProps) {
  const modelControl = useModelSettingsControl();
  const [display, saveDisplay] = useSettingsDisplayPreferences();
  const [dialogProviderId, setDialogProviderId] = useState<string>();
  const [addingCustomProvider, setAddingCustomProvider] = useState(false);
  const [catalogDialog, setCatalogDialog] = useState<{
    catalogProviderId: string;
    endpointId: string;
  }>();
  const [profileDialog, setProfileDialog] = useState<{
    endpointId: string;
    profileId?: string;
  }>();
  const [busyAction, setBusyAction] = useState<string>();
  const [profilePendingDeletion, setProfilePendingDeletion] = useState<ModelProfileConfig>();
  const profileDeletionTriggerRef = useRef<HTMLElement>(null);
  const modelSectionHeadingRef = useRef<HTMLHeadingElement>(null);
  const providers = useMemo(
    () => buildProviderViews(modelControl.settings, modelControl.catalog),
    [modelControl.catalog, modelControl.settings],
  );
  const selectedProvider =
    providers.find((provider) => provider.id === display.modelProviderId) ?? providers[0];
  const dialogProvider = providers.find((provider) => provider.id === dialogProviderId);
  const connectionFor = (provider: ProviderConfigurationView | undefined) => {
    if (provider === undefined) return undefined;
    const savedConnection = display.modelConnectionByProvider[provider.id];
    return (
      provider.endpoints.find((endpoint) => endpoint.id === savedConnection) ??
      provider.endpoints[0]
    );
  };
  const targetEndpoint = connectionFor(selectedProvider);
  const targetModels =
    targetEndpoint === undefined || modelControl.settings === undefined
      ? []
      : targetEndpoint.model_order.flatMap((profileId) => {
          const profile = modelControl.settings?.registry.models[profileId];
          return profile === undefined ? [] : [profile];
        });
  const writesUnavailable = modelControl.writeUnavailableReason !== undefined;
  // Each provider keeps its own choice; its connections share it.
  const onlyReady =
    selectedProvider !== undefined && display.configuredOnlyProviders.includes(selectedProvider.id);
  const setOnlyReady = (on: boolean) => {
    if (selectedProvider === undefined) return;
    const provider = selectedProvider.id;
    const otherProviders = display.configuredOnlyProviders.filter((id) => id !== provider);
    saveDisplay({ configuredOnlyProviders: on ? [...otherProviders, provider] : otherProviders });
  };
  const [catalogRefreshing, setCatalogRefreshing] = useState(false);
  const [modelQuery, setModelQuery] = useState('');
  const query = modelQuery.trim().toLowerCase();
  const filtering = onlyReady || query !== '';
  const shownModels = targetModels.filter(
    (model) => (!onlyReady || isProfileReady(model)) && profileMatches(model, query),
  );

  useEffect(() => {
    if (modelControl.error === undefined) {
      onWithdrawNotice?.('model-settings:');
      return;
    }
    onNotify({
      dedupeKey: `model-settings:${modelControl.error}`,
      message: userMessage(modelControl.error, '模型配置暂不可用，请检查设置与连接。'),
      title: '模型配置不可用',
      tone: 'danger',
    });
  }, [modelControl.error, onNotify, onWithdrawNotice]);

  const selectProvider = (provider: ProviderCatalogItem) => {
    saveDisplay({ modelProviderId: provider.id });
  };

  const refreshCatalog = async () => {
    if (catalogRefreshing) return;
    setCatalogRefreshing(true);
    const failure = await modelControl.refreshCatalog();
    setCatalogRefreshing(false);
    if (failure !== undefined)
      onNotify({
        dedupeKey: `models.dev 目录未刷新:${failure.code}`,
        message: failure.message,
        title: 'models.dev 目录未刷新',
        tone: 'danger',
      });
  };

  const configureProvider = (provider: ProviderCatalogItem) => {
    selectProvider(provider);
    setDialogProviderId(provider.id);
  };

  const setDefaultProfile = async (profile: ModelProfileConfig) => {
    if (busyAction !== undefined || writesUnavailable) return;
    setBusyAction(`default:${profile.id}`);
    const result = await window.aura.v1.setDefaultModel({
      expected_revision: modelControl.settings?.aura_revision ?? '0',
      model_profile_id: profile.id,
    });
    setBusyAction(undefined);
    if (!result.ok) {
      notifyFailure(onNotify, '默认模型未更新', result.error);
      return;
    }
    await modelControl.refresh();
    onNotify({
      dedupeKey: `default-model:${profile.id}:${result.value.revision}`,
      message: `${profile.display_name} 只会作为后续新会话的默认模型；现有会话绑定保持不变。`,
      title: '默认模型已更新',
      tone: 'success',
    });
  };

  const toggleProfileSubagents = async (profile: ModelProfileConfig) => {
    if (busyAction !== undefined || modelControl.settings === undefined || writesUnavailable)
      return;
    setBusyAction(`subagent:${profile.id}`);
    const result = await window.aura.v1.setModelProfileSubagentAvailability({
      available_for_subagents: !profile.available_for_subagents,
      expected_revision: modelControl.settings.models_revision,
      model_profile_id: profile.id,
    });
    setBusyAction(undefined);
    if (!result.ok) {
      notifyFailure(onNotify, '子 Agent 可用性未更新', result.error);
      return;
    }
    await modelControl.refresh();
  };

  const toggleProviderSubagents = async (provider: ProviderConfigurationView) => {
    if (
      busyAction !== undefined ||
      modelControl.settings === undefined ||
      provider.localProvider === undefined ||
      provider.models.length === 0 ||
      writesUnavailable
    ) {
      return;
    }
    const enable = provider.models.some((profile) => !profile.available_for_subagents);
    setBusyAction(`provider-subagent:${provider.localProvider.id}`);
    const result = await window.aura.v1.setProviderModelsSubagentAvailability({
      available_for_subagents: enable,
      expected_revision: modelControl.settings.models_revision,
      provider_id: provider.localProvider.id,
    });
    setBusyAction(undefined);
    if (!result.ok) {
      notifyFailure(onNotify, '服务商的子 Agent 设置未更新', result.error);
      return;
    }
    // Models without an output limit are left as they were; the user is told which.
    const skipped = result.value.skipped_model_profile_ids ?? [];
    if (skipped.length)
      onNotify({
        dedupeKey: `provider-subagent-skipped:${provider.localProvider.id}:${result.value.revision}`,
        message: `没有设最大输出上限，未允许子 Agent 使用：${provider.models
          .filter((profile) => skipped.includes(profile.id))
          .map((profile) => profile.display_name)
          .join('、')}`,
        title: '部分模型未允许',
        tone: 'warning',
      });
    await modelControl.refresh();
  };

  const deleteProfile = async (profile: ModelProfileConfig) => {
    if (busyAction !== undefined || modelControl.settings === undefined || writesUnavailable) {
      return;
    }
    setBusyAction(`delete:${profile.id}`);
    const result = await window.aura.v1.deleteModelProfile({
      expected_revision: modelControl.settings.models_revision,
      model_profile_id: profile.id,
    });
    setBusyAction(undefined);
    if (!result.ok) {
      setProfilePendingDeletion(undefined);
      notifyFailure(onNotify, '模型配置未删除', result.error);
      return;
    }
    await modelControl.refresh();
    onNotify({
      dedupeKey: `model-profile-deleted:${profile.id}:${result.value.revision}`,
      message: `${profile.display_name} 已从本地模型配置删除。`,
      title: '模型配置已删除',
      tone: 'success',
    });
    setProfilePendingDeletion(undefined);
  };

  const applyCatalogUpdate = async (profile: ModelProfileConfig) => {
    if (
      busyAction !== undefined ||
      modelControl.settings === undefined ||
      writesUnavailable ||
      profile.catalog === null ||
      profile.catalog === undefined
    ) {
      return;
    }
    setBusyAction(`catalog-update:${profile.id}`);
    const pageResult = await window.aura.v1.getModelCatalogProvider({
      after_model_id: null,
      catalog_provider_id: profile.catalog.catalog_provider_id,
      expected_catalog_revision: modelControl.catalog?.catalog_revision ?? null,
      limit: 100,
      query: profile.catalog.catalog_model_id,
    });
    if (!pageResult.ok) {
      setBusyAction(undefined);
      notifyFailure(onNotify, '目录信息未读取', pageResult.error, true);
      return;
    }
    const catalogModel = pageResult.value.models.find(
      (view) => view.model.catalog_model_id === profile.catalog?.catalog_model_id,
    );
    if (catalogModel === undefined) {
      setBusyAction(undefined);
      onNotify({
        dedupeKey: `catalog-model-missing:${profile.id}`,
        message: CATALOG_ENTRY_MISSING,
        title: '目录里没有这个模型',
        tone: 'warning',
      });
      return;
    }
    const result = await window.aura.v1.applyModelProfileCatalogUpdate({
      expected_catalog_object_hash: catalogModel.catalog_object_hash,
      expected_catalog_revision: pageResult.value.catalog_revision,
      expected_model_profile_revision: profile.revision,
      expected_models_revision: modelControl.settings.models_revision,
      model_profile_id: profile.id,
    });
    setBusyAction(undefined);
    if (!result.ok) {
      notifyFailure(onNotify, '目录信息未更新', result.error);
      return;
    }
    await modelControl.refresh();
    onNotify({
      dedupeKey: `catalog-model-updated:${profile.id}:${result.value.revision}`,
      message: CATALOG_ENTRY_UPDATED,
      title: '目录信息已更新',
      tone: 'success',
    });
  };

  if (modelControl.loading && modelControl.settings === undefined) {
    return (
      <div className="model-settings model-settings-loading" role="status">
        <AppIcon name="model" size={25} />
        <strong>正在读取 Core 模型配置…</strong>
      </div>
    );
  }

  if (modelControl.settings === undefined) {
    return (
      <div className="model-settings model-settings-loading" role="alert">
        <AppIcon name="warning" size={25} />
        <strong>无法读取模型配置</strong>
        <p>{userMessage(modelControl.error, '模型配置暂未取得，请检查设置与连接。')}</p>
        <button onClick={() => void modelControl.refresh()} type="button">
          重试
        </button>
      </div>
    );
  }

  const settings = modelControl.settings;
  const catalogState = modelControl.catalog?.state;
  const catalogBusy =
    catalogRefreshing ||
    catalogState === 'refreshing_with_lkg' ||
    catalogState === 'loading_without_cache';

  return (
    <div className="model-settings">
      <div className="settings-content-heading">
        <div>
          <h2>模型配置</h2>
        </div>
        <button
          aria-busy={catalogBusy}
          aria-label="刷新 models.dev 目录"
          className="button is-small model-catalog-refresh"
          onClick={() => void refreshCatalog()}
          title="刷新 models.dev 目录"
          type="button"
        >
          <AppIcon name="refresh" size={14} />
        </button>
      </div>

      {modelControl.writeUnavailableReason === undefined ? null : (
        <div className="model-settings-reconnect-note" role="status">
          <AppIcon name="activity" size={15} />
          <span>{modelControl.writeUnavailableReason}</span>
        </div>
      )}

      <ProviderCatalog
        catalogUnavailable={
          catalogState === 'unavailable'
            ? catalogUnavailableText(modelControl.catalog?.last_error_code)
            : modelControl.catalog === undefined
              ? modelControl.catalogError
              : undefined
        }
        onAddCustom={() => setAddingCustomProvider(true)}
        onConfigure={configureProvider}
        onSelect={selectProvider}
        providers={providers}
        selectedProviderId={selectedProvider?.id}
        writeUnavailableReason={modelControl.writeUnavailableReason}
      />

      <section aria-labelledby="provider-models-heading" className="provider-models-section">
        <div className="section-heading-row">
          <div className="model-section-title">
            <h3 id="provider-models-heading" ref={modelSectionHeadingRef} tabIndex={-1}>
              {selectedProvider === undefined ? '已配置模型' : `${selectedProvider.name} 模型`}
            </h3>
            <span className="model-section-count">
              {filtering ? `${shownModels.length}/${targetModels.length}` : targetModels.length} 个
            </span>
          </div>
          <ModelSectionToolbar
            filter={
              targetModels.length === 0
                ? undefined
                : {
                    onlyReady,
                    onOnlyReady: setOnlyReady,
                    query: modelQuery,
                    onQuery: setModelQuery,
                  }
            }
            canBrowse={
              selectedProvider?.catalogProviderId !== undefined && targetEndpoint !== undefined
            }
            endpointId={targetEndpoint?.id}
            endpoints={selectedProvider?.endpoints ?? []}
            onAdd={
              targetEndpoint === undefined
                ? undefined
                : () => setProfileDialog({ endpointId: targetEndpoint.id })
            }
            onBrowse={() => {
              if (selectedProvider?.catalogProviderId !== undefined && targetEndpoint)
                setCatalogDialog({
                  catalogProviderId: selectedProvider.catalogProviderId,
                  endpointId: targetEndpoint.id,
                });
            }}
            onEndpoint={(id) => {
              if (selectedProvider)
                saveDisplay({
                  modelConnectionByProvider: {
                    ...display.modelConnectionByProvider,
                    [selectedProvider.id]: id,
                  },
                });
            }}
            subagents={
              selectedProvider?.localProvider === undefined || selectedProvider.models.length === 0
                ? undefined
                : {
                    allow: selectedProvider.models.some(
                      (profile) => !profile.available_for_subagents,
                    ),
                    disabled: busyAction !== undefined || writesUnavailable,
                    run: () => void toggleProviderSubagents(selectedProvider),
                  }
            }
            writeUnavailableReason={modelControl.writeUnavailableReason}
          />
        </div>

        {selectedProvider === undefined || targetModels.length === 0 ? (
          <div className="model-empty-state provider-model-empty">
            <AppIcon name="model" size={28} />
            <strong>尚无已配置模型</strong>
            <p>先添加 Provider 和 Endpoint；发现不可用时仍可手工创建 Model Profile。</p>
          </div>
        ) : shownModels.length === 0 ? (
          <div className="model-empty-state provider-model-empty">
            <strong>没有符合的模型</strong>
          </div>
        ) : (
          <div className="model-card-grid">
            {shownModels.map((model) => {
              const endpoint = targetEndpoint;
              if (endpoint === undefined) return null;
              const credential = modelControl.settings?.active_credentials.find(
                (item) => item.endpoint_id === endpoint.id && item.slot_name === 'api-key',
              );
              return (
                <ModelProfileCard
                  busy={busyAction !== undefined}
                  credentialState={credential?.state}
                  endpoint={endpoint}
                  isDefault={settings.default_model_profile_id === model.id}
                  key={model.id}
                  model={model}
                  onApplyCatalogUpdate={applyCatalogUpdate}
                  onDeleteRequest={(profile, trigger) => {
                    profileDeletionTriggerRef.current = trigger;
                    setProfilePendingDeletion(profile);
                  }}
                  onEdit={(profile) =>
                    setProfileDialog({ endpointId: endpoint.id, profileId: profile.id })
                  }
                  onNotify={onNotify}
                  onSetDefault={setDefaultProfile}
                  onToggleSubagents={toggleProfileSubagents}
                  writeUnavailableReason={modelControl.writeUnavailableReason}
                />
              );
            })}
          </div>
        )}
      </section>

      <NetworkProxySection />

      {dialogProvider === undefined ? null : (
        <ProviderConnectionDialog
          onClose={() => setDialogProviderId(undefined)}
          onBrowseCatalog={() => {
            if (dialogProvider.catalogProviderId === undefined) return;
            // A provider not added yet has no connection; its model list opens to read
            // (the button did nothing before).
            setCatalogDialog({
              catalogProviderId: dialogProvider.catalogProviderId,
              endpointId: connectionFor(dialogProvider)?.id ?? '',
            });
          }}
          onNotify={onNotify}
          onSaved={modelControl.refresh}
          provider={dialogProvider}
          settings={settings}
          writeUnavailableReason={modelControl.writeUnavailableReason}
        />
      )}
      {addingCustomProvider ? (
        <CustomProviderDialog
          expectedRevision={modelControl.settings.models_revision}
          onClose={() => setAddingCustomProvider(false)}
          onNotify={onNotify}
          onSaved={modelControl.refresh}
          writeUnavailableReason={modelControl.writeUnavailableReason}
        />
      ) : null}
      {catalogDialog === undefined || selectedProvider === undefined ? null : (
        <CatalogModelDialog
          catalogProviderId={catalogDialog.catalogProviderId}
          endpoints={selectedProvider.endpoints}
          initialEndpointId={catalogDialog.endpointId}
          onClose={() => setCatalogDialog(undefined)}
          onNotify={onNotify}
          onSaved={modelControl.refresh}
          settings={modelControl.settings}
          writeUnavailableReason={modelControl.writeUnavailableReason}
        />
      )}
      {profileDialog === undefined
        ? null
        : (() => {
            const endpoint = settings.registry.endpoints[profileDialog.endpointId];
            if (endpoint === undefined) return null;
            const profile =
              profileDialog.profileId === undefined
                ? undefined
                : settings.registry.models[profileDialog.profileId];
            return (
              <ModelProfileDialog
                endpoint={endpoint}
                onClose={() => setProfileDialog(undefined)}
                onNotify={onNotify}
                onSaved={modelControl.refresh}
                {...(profile === undefined ? {} : { profile })}
                settings={settings}
                writeUnavailableReason={modelControl.writeUnavailableReason}
              />
            );
          })()}
      {profilePendingDeletion === undefined ? null : (
        <ConfirmationDialog
          busy={busyAction === `delete:${profilePendingDeletion.id}`}
          confirmLabel="删除 Model Profile"
          description={`将删除 Model Profile“${profilePendingDeletion.display_name}”；现有相关绑定会按 Core 规则清理。`}
          fallbackFocusRef={modelSectionHeadingRef}
          onCancel={() => setProfilePendingDeletion(undefined)}
          onConfirm={() => void deleteProfile(profilePendingDeletion)}
          returnFocusRef={profileDeletionTriggerRef}
          title="确认删除模型配置"
          titleId={`delete-model-profile-${profilePendingDeletion.id}`}
        />
      )}
    </div>
  );
}

function buildProviderViews(
  settings: ModelSettingsProjection | undefined,
  catalog: ProviderCatalogProjection | undefined,
): ProviderConfigurationView[] {
  if (settings === undefined) return [];
  const providers = new Map<string, ProviderConfigurationView>();
  const localProviders = Object.values(settings.registry.providers).filter(
    (provider): provider is NonNullable<typeof provider> => provider !== undefined,
  );
  const models = Object.values(settings.registry.models).filter(
    (model): model is NonNullable<typeof model> => model !== undefined,
  );

  for (const card of catalog?.cards ?? []) {
    const localProvider =
      (card.local_provider_id === null || card.local_provider_id === undefined
        ? undefined
        : settings.registry.providers[card.local_provider_id]) ??
      localProviders.find(
        (provider) =>
          provider.source.type === 'models_dev' &&
          provider.source.catalog_provider_id === card.catalog_provider_id,
      );
    const providerEndpoints =
      localProvider === undefined
        ? []
        : localProvider.endpoint_order.flatMap((endpointId) => {
            const endpoint = settings.registry.endpoints[endpointId];
            return endpoint === undefined ? [] : [endpoint];
          });
    const endpointIds = new Set(providerEndpoints.map((endpoint) => endpoint.id));
    providers.set(card.catalog_provider_id, {
      catalogModelCount: card.catalog_model_count,
      catalogProviderId: card.catalog_provider_id,
      configurationClass: card.configuration_class,
      ...(card.connection_defaults === null || card.connection_defaults === undefined
        ? {}
        : { connectionDefaults: card.connection_defaults }),
      configuredEndpointCount: providerEndpoints.length,
      endpoints: providerEndpoints,
      id: card.catalog_provider_id,
      initials: providerInitials(card.display_name),
      ...(card.icon_ref === null || card.icon_ref === undefined
        ? {}
        : {
            iconUrl: `aura://app/provider-icon/${card.icon_ref.artifact_id}/${card.icon_ref.content_hash.hex}.png`,
          }),
      localProvider,
      models: models.filter((model) => endpointIds.has(model.endpoint_id)),
      name: card.display_name,
      source: 'models.dev',
    });
  }

  for (const providerId of settings.registry.provider_order) {
    const provider = settings.registry.providers[providerId];
    if (provider === undefined) continue;
    const catalogId =
      provider.source.type === 'models_dev' ? provider.source.catalog_provider_id : undefined;
    if (catalogId !== undefined && providers.has(catalogId)) continue;
    const providerEndpoints = provider.endpoint_order.flatMap((endpointId) => {
      const endpoint = settings.registry.endpoints[endpointId];
      return endpoint === undefined ? [] : [endpoint];
    });
    const endpointIds = new Set(providerEndpoints.map((endpoint) => endpoint.id));
    providers.set(provider.id, {
      catalogModelCount: 0,
      ...(catalogId === undefined ? {} : { catalogProviderId: catalogId }),
      configurationClass: 'manual_configuration',
      configuredEndpointCount: providerEndpoints.length,
      endpoints: providerEndpoints,
      id: provider.id,
      initials: providerInitials(provider.display_name),
      localProvider: provider,
      models: models.filter((model) => endpointIds.has(model.endpoint_id)),
      name: provider.display_name,
      source: provider.source.type === 'models_dev' ? 'models.dev' : 'custom',
    });
  }

  const views = [...providers.values()];
  const localOrder = new Map(
    settings.registry.provider_order.map((providerId, index) => [providerId, index]),
  );
  const configured = views
    .filter((provider) => provider.localProvider !== undefined)
    .sort(
      (left, right) =>
        (localOrder.get(left.localProvider?.id ?? '') ?? Number.MAX_SAFE_INTEGER) -
        (localOrder.get(right.localProvider?.id ?? '') ?? Number.MAX_SAFE_INTEGER),
    );
  return [...configured, ...views.filter((provider) => provider.localProvider === undefined)];
}

const notifyFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  error: { code: string; message: string },
  read = false,
) => {
  notify({
    dedupeKey: `${title}:${error.code}`,
    message: modelFailureText(error, read),
    title,
    tone: 'danger',
  });
};
