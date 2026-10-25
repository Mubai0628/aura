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
import { ModelProfileDialog } from './ModelProfileDialog';
import type { ProviderCatalogItem, ProviderConfigurationView } from './model-settings-types';
import { providerInitials } from './model-settings-types';
import type { NotificationInput } from './notification-center-store';
import { useModelSettingsControl } from './model-settings-store';
import { ProviderCatalog } from './ProviderCatalog';
import { ProviderConnectionDialog } from './ProviderConnectionDialog';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';

interface ModelSettingsProps {
  onNotify: (notification: NotificationInput) => void;
  /** Withdraws this page's "model settings unavailable" notice once they are available again. */
  onWithdrawNotice?: (prefix: string) => void;
}

export function ModelSettings({ onNotify, onWithdrawNotice }: ModelSettingsProps) {
  const modelControl = useModelSettingsControl();
  const [selectedProviderId, setSelectedProviderId] = useState<string>();
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
  const [targetEndpointByProvider, setTargetEndpointByProvider] = useState<Record<string, string>>(
    {},
  );
  const [busyAction, setBusyAction] = useState<string>();
  const [profilePendingDeletion, setProfilePendingDeletion] = useState<ModelProfileConfig>();
  const profileDeletionTriggerRef = useRef<HTMLElement>(null);
  const modelSectionHeadingRef = useRef<HTMLHeadingElement>(null);
  const providers = useMemo(
    () => buildProviderViews(modelControl.settings, modelControl.catalog),
    [modelControl.catalog, modelControl.settings],
  );
  const selectedProvider =
    providers.find((provider) => provider.id === selectedProviderId) ?? providers[0];
  const dialogProvider = providers.find((provider) => provider.id === dialogProviderId);
  const selectedTargetEndpointId =
    selectedProvider === undefined ? undefined : targetEndpointByProvider[selectedProvider.id];
  const targetEndpoint =
    selectedProvider?.endpoints.find((endpoint) => endpoint.id === selectedTargetEndpointId) ??
    selectedProvider?.endpoints[0];
  const targetModels =
    targetEndpoint === undefined || modelControl.settings === undefined
      ? []
      : targetEndpoint.model_order.flatMap((profileId) => {
          const profile = modelControl.settings?.registry.models[profileId];
          return profile === undefined ? [] : [profile];
        });
  const writesUnavailable = modelControl.writeUnavailableReason !== undefined;

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
    setSelectedProviderId(provider.id);
  };

  const configureProvider = (provider: ProviderCatalogItem) => {
    setSelectedProviderId(provider.id);
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
      notifyFailure(onNotify, 'Provider 子 Agent 策略未更新', result.error);
      return;
    }
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
      notifyFailure(onNotify, 'Model Profile 未删除', result.error);
      return;
    }
    await modelControl.refresh();
    onNotify({
      dedupeKey: `model-profile-deleted:${profile.id}:${result.value.revision}`,
      message: `${profile.display_name} 已从本地模型配置删除。`,
      title: 'Model Profile 已删除',
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
      notifyFailure(onNotify, '目录快照未读取', pageResult.error);
      return;
    }
    const catalogModel = pageResult.value.models.find(
      (view) => view.model.catalog_model_id === profile.catalog?.catalog_model_id,
    );
    if (catalogModel === undefined) {
      setBusyAction(undefined);
      onNotify({
        dedupeKey: `catalog-model-missing:${profile.id}`,
        message: '当前目录 Revision 中没有该 Provider 原始模型 ID；Aura 不会进行模糊匹配。',
        title: '目录模型未找到',
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
      notifyFailure(onNotify, '目录快照未更新', result.error);
      return;
    }
    await modelControl.refresh();
    onNotify({
      dedupeKey: `catalog-model-updated:${profile.id}:${result.value.revision}`,
      message: '仅 Catalog-owned Snapshot 已更新；用户修正、备注和执行字段保持不变。',
      title: '目录快照已更新',
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
  const catalogLabel = catalogStatusLabel(modelControl.catalog, modelControl.catalogError);

  return (
    <div className="model-settings">
      <div className="settings-content-heading">
        <div>
          <h2 title={`配置修订 ${modelControl.settings.models_revision}`}>模型配置</h2>
        </div>
      </div>

      {modelControl.writeUnavailableReason === undefined ? null : (
        <div className="model-settings-reconnect-note" role="status">
          <AppIcon name="activity" size={15} />
          <span>{modelControl.writeUnavailableReason}</span>
        </div>
      )}

      <ProviderCatalog
        catalogLabel={catalogLabel}
        onAddCustom={() => setAddingCustomProvider(true)}
        onConfigure={configureProvider}
        onRefresh={() => void modelControl.refreshCatalog()}
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
            <span className="model-section-count">{targetModels.length} 个</span>
          </div>
          <ModelSectionToolbar
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
                setTargetEndpointByProvider((current) => ({
                  ...current,
                  [selectedProvider.id]: id,
                }));
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
        ) : (
          <div className="model-card-grid">
            {targetModels.map((model) => {
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

      {dialogProvider === undefined ? null : (
        <ProviderConnectionDialog
          onClose={() => setDialogProviderId(undefined)}
          onBrowseCatalog={() => {
            if (dialogProvider.catalogProviderId === undefined) return;
            // A provider not added yet has no connection; its model list opens to read
            // (the button did nothing before).
            setCatalogDialog({
              catalogProviderId: dialogProvider.catalogProviderId,
              endpointId:
                targetEndpointByProvider[dialogProvider.id] ??
                dialogProvider.endpoints[0]?.id ??
                '',
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

const catalogStatusLabel = (
  catalog: ProviderCatalogProjection | undefined,
  error: string | undefined,
): string => {
  if (catalog === undefined) return error === undefined ? '目录读取中' : '目录不可用';
  switch (catalog.state) {
    case 'fresh':
      return 'models.dev 目录 · 最新';
    case 'stale':
      return 'models.dev 目录 · 缓存';
    case 'loading_without_cache':
      return 'models.dev 目录 · 正在加载';
    case 'refreshing_with_lkg':
      return 'models.dev 目录 · 正在刷新';
    case 'refresh_failed_with_lkg':
      return 'models.dev 目录 · 刷新失败，使用缓存';
    case 'unavailable':
      return 'models.dev 目录 · 不可用';
  }
};

const notifyFailure = (
  notify: (notification: NotificationInput) => void,
  title: string,
  error: { code: string; message: string },
) => {
  notify({
    dedupeKey: `${title}:${error.code}`,
    message: userMessage(error, MODEL_FAILURE_FALLBACK),
    title,
    tone: 'danger',
  });
};
