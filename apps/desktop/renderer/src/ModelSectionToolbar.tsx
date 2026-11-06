import type { ModelEndpointConfig } from '@aura/contracts/generated';
import { AppIcon } from './app-icons';
import { ToggleSwitch } from './ui/ToggleSwitch';
import { useContextMenu } from './ui/useContextMenu';

/** Narrowing the provider's models by words and to the ones ready to use. */
export interface ModelSectionFilter {
  onlyReady: boolean;
  onOnlyReady: (onlyReady: boolean) => void;
  query: string;
  onQuery: (query: string) => void;
}

/**
 * One provider's models are found by name and narrowed to the ones ready to use, and chosen
 * through its connection; adding a model from the list or by hand stays in view, and switching
 * every model's sub-agent use is in the 更多 menu.
 */
export function ModelSectionToolbar({
  filter,
  endpoints,
  endpointId,
  onEndpoint,
  canBrowse,
  onBrowse,
  onAdd,
  writeUnavailableReason,
  subagents,
}: {
  /** Absent while the provider has no models to narrow. */
  filter: ModelSectionFilter | undefined;
  endpoints: readonly ModelEndpointConfig[];
  endpointId: string | undefined;
  onEndpoint: (id: string) => void;
  canBrowse: boolean;
  onBrowse: () => void;
  onAdd: (() => void) | undefined;
  writeUnavailableReason: string | undefined;
  /** Switching every model of the provider, when it has models. */
  subagents?: { allow: boolean; disabled: boolean; run: () => void } | undefined;
}) {
  const menu = useContextMenu();
  return (
    <div className="model-section-actions">
      {filter === undefined ? null : (
        <>
          <label className="model-section-search field-frame">
            <AppIcon name="search" size={16} />
            <input
              aria-label="搜索模型"
              onChange={(event) => filter.onQuery(event.target.value)}
              placeholder="搜索模型"
              type="search"
              value={filter.query}
            />
          </label>
          <label className="model-section-filter">
            <span>只看已配置</span>
            <ToggleSwitch
              checked={filter.onlyReady}
              label="只看已配置"
              onCheckedChange={filter.onOnlyReady}
            />
          </label>
        </>
      )}
      {endpoints.length === 0 ? null : (
        <label className="model-endpoint-select">
          <span>连接</span>
          <select
            aria-label="Model Profile 目标 Endpoint"
            onChange={(event) => onEndpoint(event.target.value)}
            value={endpointId ?? ''}
          >
            {endpoints.map((endpoint) => (
              <option key={endpoint.id} value={endpoint.id}>
                {endpoint.display_name}
              </option>
            ))}
          </select>
        </label>
      )}
      {canBrowse ? (
        <button className="button is-small" onClick={onBrowse} type="button">
          浏览模型列表
        </button>
      ) : null}
      {onAdd === undefined ? null : (
        <span
          className="disabled-action-wrapper"
          tabIndex={writeUnavailableReason === undefined ? -1 : 0}
        >
          <button
            className="button is-small"
            disabled={writeUnavailableReason !== undefined}
            onClick={onAdd}
            type="button"
          >
            <AppIcon name="plus" size={14} />
            手工添加模型
          </button>
          {writeUnavailableReason === undefined ? null : (
            <span role="tooltip">{writeUnavailableReason}</span>
          )}
        </span>
      )}
      {subagents === undefined ? null : (
        <button
          aria-label="这个提供商的更多操作"
          className="icon-button"
          onClick={(event) =>
            menu.open(event, '这个提供商的更多操作', [
              {
                label: subagents.allow ? '全部允许子 Agent 使用' : '全部关闭子 Agent 使用',
                disabled: subagents.disabled,
                run: subagents.run,
              },
            ])
          }
          title="更多操作"
          type="button"
        >
          <AppIcon name="more" size={16} />
        </button>
      )}
      {menu.menu}
    </div>
  );
}
