import { AppIcon } from '../app-icons';
import { endpointApiFormatLabel } from '../model-settings-types';
import { changedAt, connectionRoles } from './connection-roles';
import { credentialStatusLabel } from './credential-status';
import type { ProviderConnection } from './useProviderConnection';

/** A provider's name and its saved connections. */
export function ConnectionListView({ connection }: { connection: ProviderConnection }) {
  const {
    provider,
    settings,
    busy,
    setPendingDeletion,
    setProviderDisplayNameDraft,
    addConnectionButtonRef,
    deletionTriggerRef,
    providerDisplayName,
    writesDisabled,
    beginEditing,
    renameProvider,
    setConnections,
  } = connection;
  // The dialog shows this view only for a provider already saved in Aura.
  const localProvider = provider.localProvider;
  if (localProvider === undefined) return null;
  const roles = connectionRoles(localProvider.default_endpoint_id, provider.endpoints);
  return (
    <div className="provider-connections">
      <div className="provider-maintenance-row">
        <label>
          <span>Provider 名称</span>
          <input
            onChange={(event) =>
              setProviderDisplayNameDraft({
                providerId: provider.id,
                value: event.target.value,
              })
            }
            value={providerDisplayName}
          />
        </label>
        <button
          disabled={
            busy ||
            writesDisabled ||
            providerDisplayName.trim().length === 0 ||
            providerDisplayName === localProvider.display_name
          }
          onClick={() => void renameProvider()}
          type="button"
        >
          保存名称
        </button>
      </div>
      <div className="provider-connection-heading">
        <div>
          <h3>Endpoint / 连接</h3>
        </div>
        <button
          disabled={writesDisabled}
          onClick={() => beginEditing()}
          ref={addConnectionButtonRef}
          type="button"
        >
          <AppIcon name="plus" size={15} />
          添加连接
        </button>
      </div>

      {provider.endpoints.length > 0 ? (
        <div className="connection-list">
          {provider.endpoints.map((endpoint) => {
            const credential = settings.active_credentials.find(
              (item) => item.endpoint_id === endpoint.id && item.slot_name === 'api-key',
            );
            const dormant = settings.dormant_credentials.find(
              (item) => item.endpoint_id === endpoint.id && item.slot_name === 'api-key',
            );
            const role = roles.get(endpoint.id);
            return (
              <div className="connection-item" key={endpoint.id}>
                <button
                  className="connection-preview"
                  onClick={() => beginEditing(endpoint)}
                  type="button"
                >
                  <span>
                    <strong>{endpoint.display_name}</strong>
                    <small>{endpointApiFormatLabel(endpoint.api_format)}</small>
                    {/* */}
                    <small>修改于 {changedAt(endpoint.updated_at)}</small>
                  </span>
                  <span className="draft-state">
                    {credentialStatusLabel(endpoint, credential, dormant)}
                  </span>
                  <AppIcon name="chevron-right" />
                </button>
                {role !== undefined ? (
                  <div className="connection-routing">
                    {role.kind === 'default' ? (
                      <span className="status-pill">默认</span>
                    ) : (
                      <>
                        <span className="status-pill" data-tone="neutral">
                          备用 {role.place}
                        </span>
                        <button
                          disabled={busy || writesDisabled}
                          onClick={() => void setConnections(endpoint.id, [])}
                          type="button"
                        >
                          设为默认
                        </button>
                      </>
                    )}
                  </div>
                ) : null}
              </div>
            );
          })}
        </div>
      ) : (
        <div className="connection-empty-state">
          <AppIcon name="model" size={27} />
          <strong>尚未配置连接</strong>
          <p>添加 Endpoint 后才能请求模型目录并创建 Model Profile。</p>
        </div>
      )}
      <div className="provider-danger-row">
        <button
          className="danger-button"
          disabled={busy || writesDisabled}
          onClick={(event) => {
            deletionTriggerRef.current = event.currentTarget;
            setPendingDeletion({
              displayName: localProvider.display_name,
              kind: 'provider',
            });
          }}
          type="button"
        >
          删除 Provider
        </button>
      </div>
    </div>
  );
}
