import type {
  ModelEndpointConfig,
  ModelSettingsProjection,
  ProviderCatalogModelPage,
  ProviderCatalogModelView,
} from '@aura/contracts/generated';
import { useCallback, useEffect, useState } from 'react';

import { AppIcon } from './app-icons';
import { ModalDialog } from './ModalDialog';
import type { NotificationInput } from './notification-center-store';
import { userMessage } from './agent-session/user-message';
import { MODEL_FAILURE_FALLBACK } from './provider-connection/notices';

interface CatalogModelDialogProps {
  catalogProviderId: string;
  endpoints: readonly ModelEndpointConfig[];
  initialEndpointId: string;
  onClose: () => void;
  onNotify: (notification: NotificationInput) => void;
  onSaved: () => Promise<void>;
  settings: ModelSettingsProjection;
  writeUnavailableReason?: string | undefined;
}

export function CatalogModelDialog({
  catalogProviderId,
  endpoints,
  initialEndpointId,
  onClose,
  onNotify,
  onSaved,
  settings,
  writeUnavailableReason,
}: CatalogModelDialogProps) {
  const [queryDraft, setQueryDraft] = useState('');
  const [query, setQuery] = useState('');
  const [selectedEndpointId, setSelectedEndpointId] = useState(() =>
    endpoints.some((endpoint) => endpoint.id === initialEndpointId)
      ? initialEndpointId
      : (endpoints[0]?.id ?? ''),
  );
  const endpointId = endpoints.some((endpoint) => endpoint.id === selectedEndpointId)
    ? selectedEndpointId
    : (endpoints[0]?.id ?? '');
  const [page, setPage] = useState<ProviderCatalogModelPage>();
  const [models, setModels] = useState<ProviderCatalogModelView[]>([]);
  const [loading, setLoading] = useState(true);
  const [creatingModelId, setCreatingModelId] = useState<string>();
  const [error, setError] = useState<string>();
  const titleId = `catalog-models-${catalogProviderId}`;

  const loadPage = useCallback(
    async (afterModelId: string | undefined, expectedCatalogRevision: string | null) => {
      setLoading(true);
      setError(undefined);
      const result = await window.aura.v1.getModelCatalogProvider({
        after_model_id: afterModelId ?? null,
        catalog_provider_id: catalogProviderId,
        expected_catalog_revision: expectedCatalogRevision,
        limit: 50,
        query: query === '' ? null : query,
      });
      setLoading(false);
      if (!result.ok) {
        setError(userMessage(result.error, '模型目录读取失败，请重试。'));
        return;
      }
      setPage(result.value);
      setModels((current) =>
        afterModelId === undefined ? result.value.models : [...current, ...result.value.models],
      );
    },
    [catalogProviderId, query],
  );

  useEffect(() => {
    let active = true;
    void window.aura.v1
      .getModelCatalogProvider({
        after_model_id: null,
        catalog_provider_id: catalogProviderId,
        expected_catalog_revision: null,
        limit: 50,
        query: query === '' ? null : query,
      })
      .then((result) => {
        if (!active) return;
        setLoading(false);
        if (!result.ok) {
          setError(userMessage(result.error, '模型目录读取失败，请重试。'));
          return;
        }
        setPage(result.value);
        setModels(result.value.models);
      });
    return () => {
      active = false;
    };
  }, [catalogProviderId, query]);

  const createFromCatalog = async (view: ProviderCatalogModelView) => {
    const endpoint = endpoints.find((item) => item.id === endpointId);
    if (
      endpoint === undefined ||
      page === undefined ||
      creatingModelId !== undefined ||
      writeUnavailableReason !== undefined ||
      modelAlreadyAdded(settings, endpoint.id, view.model.catalog_model_id)
    ) {
      return;
    }
    setCreatingModelId(view.model.catalog_model_id);
    const result = await window.aura.v1.createModelProfileFromCatalog({
      catalog_model_id: view.model.catalog_model_id,
      catalog_provider_id: catalogProviderId,
      endpoint_id: endpoint.id,
      expected_catalog_object_hash: view.catalog_object_hash,
      expected_catalog_revision: page.catalog_revision,
      expected_endpoint_revision: endpoint.revision,
      expected_models_revision: settings.models_revision,
    });
    setCreatingModelId(undefined);
    if (!result.ok) {
      notifyFailure(onNotify, '目录模型未添加', result.error);
      return;
    }
    await onSaved();
    onNotify({
      dedupeKey: `catalog-model-created:${result.value.value.id}`,
      message: `${result.value.value.display_name} 已复制当前 models.dev 快照并创建为本地 Model Profile。`,
      title: '目录模型已添加',
      tone: 'success',
    });
  };

  return (
    <ModalDialog
      className="provider-dialog catalog-model-dialog"
      onClose={onClose}
      titleId={titleId}
    >
      <header className="modal-header">
        <div>
          <h2 id={titleId}>{page?.provider.display_name ?? catalogProviderId} 模型目录</h2>
        </div>
        <button aria-label="关闭模型目录" className="icon-button" onClick={onClose} type="button">
          <AppIcon name="close" />
        </button>
      </header>
      <div className="provider-editor catalog-model-body">
        <form
          className="catalog-model-toolbar"
          onSubmit={(event) => {
            event.preventDefault();
            setPage(undefined);
            setModels([]);
            if (queryDraft === query) {
              void loadPage(undefined, null);
            } else {
              setLoading(true);
              setError(undefined);
              setQuery(queryDraft);
            }
          }}
        >
          {endpoints.length === 0 ? (
            <div className="catalog-model-readonly-note" role="note">
              当前只浏览目录信息；添加这个提供商并配置连接后才能添加模型。
            </div>
          ) : (
            <label>
              <span>目标 Endpoint</span>
              <select
                onChange={(event) => setSelectedEndpointId(event.target.value)}
                value={endpointId}
              >
                {endpoints.map((endpoint) => (
                  <option key={endpoint.id} value={endpoint.id}>
                    {endpoint.display_name}
                  </option>
                ))}
              </select>
            </label>
          )}
          <label className="catalog-model-search">
            <span>搜索目录模型</span>
            <span className="search-field field-frame">
              <AppIcon name="search" size={14} />
              <input
                onChange={(event) => setQueryDraft(event.target.value)}
                placeholder="按模型 ID 或名称"
                type="search"
                value={queryDraft}
              />
            </span>
          </label>
          <button disabled={loading} type="submit">
            搜索
          </button>
        </form>

        {error === undefined ? null : (
          <div className="provider-editor-note" role="alert">
            {error}
          </div>
        )}
        {writeUnavailableReason === undefined ? null : (
          <div className="model-settings-reconnect-note" role="status">
            <AppIcon name="activity" size={15} />
            <span>{writeUnavailableReason}</span>
          </div>
        )}
        {models.length === 0 && loading ? (
          <div className="model-empty-state provider-model-empty" role="status">
            正在读取 models.dev 目录…
          </div>
        ) : models.length === 0 ? (
          <div className="model-empty-state provider-model-empty">
            <strong>没有匹配的目录模型</strong>
            <p>你仍可关闭目录并手工创建 Model Profile。</p>
          </div>
        ) : (
          <div className="catalog-model-list">
            {models.map((view) => {
              const added = modelAlreadyAdded(settings, endpointId, view.model.catalog_model_id);
              return (
                <article className="catalog-model-row" key={view.model.catalog_model_id}>
                  <div>
                    <strong>{view.model.display_name}</strong>
                    <small>{view.model.catalog_model_id}</small>
                    {view.model.description === null ||
                    view.model.description === undefined ? null : (
                      <p>{view.model.description}</p>
                    )}
                  </div>
                  <dl>
                    <div>
                      <dt>上下文</dt>
                      <dd>{formatTokens(view.model.context_window_tokens)}</dd>
                    </div>
                    <div>
                      <dt>输出</dt>
                      <dd>{formatTokens(view.model.output_limit_tokens)}</dd>
                    </div>
                  </dl>
                  <span
                    className="disabled-action-wrapper"
                    tabIndex={writeUnavailableReason === undefined ? -1 : 0}
                  >
                    <button
                      disabled={
                        added ||
                        endpointId === '' ||
                        creatingModelId !== undefined ||
                        writeUnavailableReason !== undefined
                      }
                      onClick={() => void createFromCatalog(view)}
                      type="button"
                    >
                      {added
                        ? '已添加'
                        : endpointId === ''
                          ? '仅查看'
                          : creatingModelId === view.model.catalog_model_id
                            ? '添加中…'
                            : '添加'}
                    </button>
                    {writeUnavailableReason === undefined ? null : (
                      <span role="tooltip">{writeUnavailableReason}</span>
                    )}
                  </span>
                </article>
              );
            })}
          </div>
        )}

        <footer className="modal-actions">
          <span>Catalog Revision {page?.catalog_revision ?? '—'}</span>
          <span className="modal-action-spacer" />
          {page?.next_model_id === null || page?.next_model_id === undefined ? null : (
            <button
              disabled={loading}
              onClick={() => void loadPage(page.next_model_id ?? undefined, page.catalog_revision)}
              type="button"
            >
              {loading ? '加载中…' : '加载更多'}
            </button>
          )}
          <button onClick={onClose} type="button">
            完成
          </button>
        </footer>
      </div>
    </ModalDialog>
  );
}

const formatTokens = (value: number | null | undefined): string =>
  value === null || value === undefined ? '未声明' : new Intl.NumberFormat('zh-CN').format(value);

const modelAlreadyAdded = (
  settings: ModelSettingsProjection,
  endpointId: string,
  providerModelName: string,
): boolean =>
  Object.values(settings.registry.models).some(
    (profile) =>
      profile !== undefined &&
      profile.endpoint_id === endpointId &&
      profile.provider_model_name === providerModelName,
  );

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
