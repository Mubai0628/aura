import { ConfirmationDialog } from './ConfirmationDialog';
import { ModalDialog } from './ModalDialog';
import { ProviderIcon } from './ProviderIcon';
import { ProviderSetupIntro } from './ProviderSetupIntro';
import { AppIcon } from './app-icons';
import { ConnectionListView } from './provider-connection/ConnectionListView';
import { EndpointEditorForm } from './provider-connection/EndpointEditorForm';
import {
  type ProviderConnectionDialogProps,
  useProviderConnection,
} from './provider-connection/useProviderConnection';
import './model-dialogs.css';

export function ProviderConnectionDialog(props: ProviderConnectionDialogProps) {
  const connection = useProviderConnection(props);
  const {
    onBrowseCatalog,
    provider,
    writeUnavailableReason,
    editorTarget,
    busy,
    pendingDeletion,
    setPendingDeletion,
    addConnectionButtonRef,
    deletionTriggerRef,
    titleId,
    writesDisabled,
    close,
    addCatalogProvider,
    deleteProvider,
    deleteEndpoint,
  } = connection;
  return (
    <>
      <ModalDialog
        className={`provider-dialog provider-connection-dialog${provider.localProvider === undefined ? ' provider-setup-dialog' : ''}`}
        onClose={close}
        suspended={pendingDeletion !== undefined}
        titleId={titleId}
      >
        <header className="modal-header">
          <div className="provider-dialog-title">
            <span className="provider-mark" aria-hidden="true">
              <ProviderIcon fallbackInitial={provider.initials} iconUrl={provider.iconUrl} />
            </span>
            <div>
              <h2 id={titleId}>{provider.name}</h2>
              <p>
                {provider.source === 'models.dev'
                  ? `models.dev Provider ID：${provider.catalogProviderId ?? '未关联'}`
                  : '自定义 Provider'}
              </p>
            </div>
          </div>
          <button
            aria-label="关闭 Provider 配置"
            className="icon-button"
            onClick={close}
            type="button"
          >
            <AppIcon name="close" />
          </button>
        </header>

        {writeUnavailableReason === undefined ? null : (
          <div className="provider-editor-note" role="status">
            {writeUnavailableReason}
          </div>
        )}

        {provider.localProvider === undefined ? (
          <ProviderSetupIntro
            provider={provider}
            busy={busy}
            disabled={writesDisabled}
            onAdd={() => void addCatalogProvider()}
            onBrowse={onBrowseCatalog}
          />
        ) : editorTarget === undefined ? (
          <ConnectionListView connection={connection} />
        ) : (
          <EndpointEditorForm connection={connection} />
        )}
      </ModalDialog>
      {pendingDeletion === undefined ? null : (
        <ConfirmationDialog
          busy={busy}
          confirmLabel={pendingDeletion.kind === 'provider' ? '删除 Provider' : '删除 Endpoint'}
          description={
            pendingDeletion.kind === 'provider'
              ? `将永久删除 Provider“${pendingDeletion.displayName}”及其 Endpoint 和模型配置。`
              : `将永久删除 Endpoint“${pendingDeletion.displayName}”及其模型配置。`
          }
          fallbackFocusRef={
            pendingDeletion.kind === 'endpoint' ? addConnectionButtonRef : undefined
          }
          onCancel={() => setPendingDeletion(undefined)}
          onConfirm={() =>
            void (pendingDeletion.kind === 'provider' ? deleteProvider() : deleteEndpoint())
          }
          returnFocusRef={deletionTriggerRef}
          title="确认删除配置"
          titleId={`${titleId}-delete-confirmation`}
        />
      )}
    </>
  );
}
