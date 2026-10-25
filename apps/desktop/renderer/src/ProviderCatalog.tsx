import { useEffect, useMemo, useRef, useState } from 'react';

import { AppIcon } from './app-icons';
import type { ProviderCatalogItem } from './model-settings-types';
import { ProviderIcon } from './ProviderIcon';

interface ProviderCatalogProps {
  catalogLabel: string;
  onAddCustom: () => void;
  onConfigure: (provider: ProviderCatalogItem) => void;
  onSelect: (provider: ProviderCatalogItem) => void;
  onRefresh: () => void;
  providers: readonly ProviderCatalogItem[];
  selectedProviderId: string | undefined;
  writeUnavailableReason?: string | undefined;
}

const MIN_CARD_WIDTH = 215;
const GRID_GAP = 10;
const COLLAPSED_ROWS = 3;

export function ProviderCatalog({
  catalogLabel,
  onAddCustom,
  onConfigure,
  onSelect,
  onRefresh,
  providers,
  selectedProviderId,
  writeUnavailableReason,
}: ProviderCatalogProps) {
  const configured = useMemo(
    () => providers.filter((provider) => provider.localProvider !== undefined),
    [providers],
  );
  const unconfigured = useMemo(
    () => providers.filter((provider) => provider.localProvider === undefined),
    [providers],
  );

  return (
    <section aria-labelledby="provider-catalog-heading" className="provider-catalog">
      <div className="section-heading-row">
        <div>
          <h3 id="provider-catalog-heading">Provider</h3>
        </div>
        <span className="provider-catalog-status">
          {catalogLabel}
          <button aria-label="刷新 models.dev 目录" onClick={onRefresh} type="button">
            <AppIcon name="refresh" size={14} />
          </button>
        </span>
      </div>

      <ProviderGroup
        addCustom={onAddCustom}
        onConfigure={onConfigure}
        providers={configured}
        selectedProviderId={selectedProviderId}
        title="已配置 Provider"
        writeUnavailableReason={writeUnavailableReason}
        onSelect={onSelect}
      />
      <div className="provider-group-divider" />
      <ProviderGroup
        providers={unconfigured}
        onConfigure={onConfigure}
        selectedProviderId={selectedProviderId}
        title="未配置目录"
        onSelect={onSelect}
      />
    </section>
  );
}

function ProviderGroup({
  addCustom,
  onConfigure,
  onSelect,
  providers,
  selectedProviderId,
  title,
  writeUnavailableReason,
}: {
  addCustom?: () => void;
  onConfigure: (provider: ProviderCatalogItem) => void;
  onSelect: (provider: ProviderCatalogItem) => void;
  providers: readonly ProviderCatalogItem[];
  selectedProviderId: string | undefined;
  title: string;
  writeUnavailableReason?: string | undefined;
}) {
  const gridRef = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(1);
  const [expanded, setExpanded] = useState(false);
  const itemCount = providers.length + (addCustom === undefined ? 0 : 1);
  const collapsedCapacity = Math.max(columns * COLLAPSED_ROWS, 1);
  const canCollapse = itemCount > collapsedCapacity;
  const visibleProviderCount =
    !expanded && canCollapse
      ? Math.max(collapsedCapacity - 1 - (addCustom === undefined ? 0 : 1), 0)
      : providers.length;

  useEffect(() => {
    const element = gridRef.current;
    if (element === null) return;
    const update = (width: number) => {
      setColumns(Math.max(1, Math.floor((width + GRID_GAP) / (MIN_CARD_WIDTH + GRID_GAP))));
    };
    const measuredWidth = () => element.clientWidth || element.getBoundingClientRect().width;
    update(measuredWidth());
    if (typeof ResizeObserver === 'undefined') {
      const updateFromWindow = () => update(measuredWidth());
      window.addEventListener('resize', updateFromWindow);
      return () => window.removeEventListener('resize', updateFromWindow);
    }
    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry !== undefined) update(entry.contentRect.width);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  return (
    <section className="provider-group" aria-label={title}>
      <div className="provider-group-heading">
        <h4>{title}</h4>
        <span>{providers.length} 个</span>
      </div>
      <div className="provider-grid" ref={gridRef}>
        {providers.slice(0, visibleProviderCount).map((provider) => (
          <ProviderCard
            key={provider.id}
            onConfigure={onConfigure}
            onSelect={onSelect}
            provider={provider}
            selected={selectedProviderId === provider.id}
          />
        ))}
        {addCustom === undefined ? null : (
          <AddProviderCard onAdd={addCustom} reason={writeUnavailableReason} />
        )}
        {canCollapse ? (
          <button
            aria-expanded={expanded}
            className="provider-card provider-expand-card"
            onClick={() => setExpanded((current) => !current)}
            type="button"
          >
            <span className="provider-mark" aria-hidden="true">
              <AppIcon name={expanded ? 'chevron-down' : 'more'} size={17} />
            </span>
            <span className="provider-card-copy">
              <strong>{expanded ? '收起 Provider' : '查看更多'}</strong>
              <span>
                {expanded ? '恢复三行预览' : `还有 ${itemCount - collapsedCapacity + 1} 个`}
              </span>
            </span>
          </button>
        ) : null}
      </div>
    </section>
  );
}

function ProviderCard({
  onConfigure,
  onSelect,
  provider,
  selected,
}: {
  onConfigure: (provider: ProviderCatalogItem) => void;
  onSelect: (provider: ProviderCatalogItem) => void;
  provider: ProviderCatalogItem;
  selected: boolean;
}) {
  return (
    <div
      className={[
        'provider-card provider-card-shell',
        selected ? 'is-selected' : '',
        provider.configurationClass === 'catalog_information_only' &&
        provider.localProvider === undefined
          ? 'is-information-only'
          : '',
      ]
        .filter(Boolean)
        .join(' ')}
    >
      <button
        aria-pressed={selected}
        className="provider-card-select"
        onClick={() => onSelect(provider)}
        type="button"
      >
        <span className="provider-mark" aria-hidden="true">
          <ProviderIcon fallbackInitial={provider.initials} iconUrl={provider.iconUrl} />
        </span>
        <span className="provider-card-copy">
          <strong>{provider.name}</strong>
          <span>
            {provider.source === 'models.dev'
              ? `${provider.catalogModelCount} 个目录模型`
              : '手动维护目录'}
          </span>
        </span>
        <span className="provider-card-meta">
          {provider.configuredEndpointCount > 0
            ? `${provider.configuredEndpointCount} 个连接`
            : provider.localProvider === undefined
              ? provider.configurationClass === 'catalog_information_only'
                ? '仅目录信息'
                : '尚未添加'
              : '未配置连接'}
        </span>
      </button>
      <button
        aria-label={`配置 ${provider.name}`}
        className="provider-card-config"
        onClick={() => onConfigure(provider)}
        type="button"
      >
        <AppIcon name="more" size={15} />
      </button>
    </div>
  );
}

function AddProviderCard({ onAdd, reason }: { onAdd: () => void; reason?: string | undefined }) {
  return (
    <span className="disabled-action-wrapper provider-card-wrapper" tabIndex={reason ? 0 : -1}>
      <button
        className="provider-card provider-add-card"
        disabled={reason !== undefined}
        onClick={onAdd}
        type="button"
      >
        <span className="provider-mark" aria-hidden="true">
          <AppIcon name="plus" size={17} />
        </span>
        <span className="provider-card-copy">
          <strong>添加提供商</strong>
          <span>创建自定义 Provider</span>
        </span>
      </button>
      {reason === undefined ? null : <span role="tooltip">{reason}</span>}
    </span>
  );
}
