import { AppIcon } from './app-icons';
import type { ProviderConfigurationView } from './model-settings-types';

export function ProviderSetupIntro({
  provider,
  busy,
  disabled,
  onAdd,
  onBrowse,
}: {
  provider: ProviderConfigurationView;
  busy: boolean;
  disabled: boolean;
  onAdd: () => void;
  onBrowse: () => void;
}) {
  const informationOnly = provider.configurationClass === 'catalog_information_only';
  // The provider and what adding
  // it leads to, the three steps in one line, and the actions together at the end.
  return (
    <div className="provider-setup-intro">
      <div className="provider-setup-lead">
        <span className="dialog-feature-icon">
          <AppIcon name="plus" size={20} />
        </span>
        <div>
          <h3>{informationOnly ? '查看提供商目录' : '添加此提供商'}</h3>
          <p>添加 {provider.name} 后，配置连接地址和凭据，即可选择要使用的模型。</p>
        </div>
      </div>
      <ol className="provider-setup-steps" aria-label="配置步骤">
        <li>
          <span>1</span>添加提供商
        </li>
        <li>
          <span>2</span>配置连接
        </li>
        <li>
          <span>3</span>选择模型
        </li>
      </ol>
      {informationOnly ? (
        <p className="settings-form-hint">此提供商仅提供目录信息，尚不支持创建连接。</p>
      ) : null}
      <div className="provider-setup-actions">
        {provider.catalogProviderId === undefined ? null : (
          <button className="button" onClick={onBrowse} type="button">
            浏览模型列表
          </button>
        )}
        <button
          className="button is-primary"
          disabled={busy || disabled || informationOnly}
          onClick={onAdd}
          type="button"
        >
          {busy ? '添加中…' : '添加此提供商'}
        </button>
      </div>
    </div>
  );
}
