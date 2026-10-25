import type {
  EndpointCredentialState,
  ModelEndpointConfig,
  ModelProfileConfig,
} from '@aura/contracts/generated';
import { useMemo } from 'react';

import { AppIcon } from './app-icons';
import { useContextMenu, type MenuEntry } from './ui/useContextMenu';
import { useScopedModelProjection } from './model-projection-store';
import type { NotificationInput } from './notification-center-store';
import {
  AvailabilitySkeleton,
  RequestedModelAvailability,
} from './model-availability/RequestedModelAvailability';
import { useModelAvailability } from './model-availability/use-model-availability';
import './model-availability/availability.css';

export function ModelProfileCard({
  busy,
  credentialState,
  endpoint,
  isDefault,
  model,
  onApplyCatalogUpdate,
  onDeleteRequest,
  onEdit,
  onNotify,
  onSetDefault,
  onToggleSubagents,
  writeUnavailableReason,
}: {
  busy: boolean;
  credentialState: EndpointCredentialState | undefined;
  endpoint: ModelEndpointConfig;
  isDefault: boolean;
  model: ModelProfileConfig;
  onApplyCatalogUpdate: (profile: ModelProfileConfig) => Promise<void>;
  onDeleteRequest: (profile: ModelProfileConfig, trigger: HTMLElement) => void;
  onEdit: (profile: ModelProfileConfig) => void;
  onNotify: (notification: NotificationInput) => void;
  onSetDefault: (profile: ModelProfileConfig) => Promise<void>;
  onToggleSubagents: (profile: ModelProfileConfig) => Promise<void>;
  writeUnavailableReason: string | undefined;
}) {
  const route = useMemo(
    () =>
      ({
        projection_kind: 'model_profile',
        projection_scope: { model_profile_id: model.id, type: 'model_profile' },
      }) as const,
    [model.id],
  );
  const change = useScopedModelProjection(route);
  const activeAvailability = change?.payload.active_availability_test;
  const operation = useModelAvailability(model, endpoint, onNotify);
  const testing =
    operation.testing || (activeAvailability !== null && activeAvailability !== undefined);
  const prerequisite =
    writeUnavailableReason ?? availabilityPrerequisite(endpoint, credentialState);
  const menu = useContextMenu();

  const locked = busy || writeUnavailableReason !== undefined;
  // The card keeps the two actions a model is used
  // for in view; the rest is one "更多" menu away.
  const more = (trigger: HTMLElement): MenuEntry[] => [
    {
      label: model.available_for_subagents ? '关闭子 Agent' : '允许子 Agent',
      disabled: locked,
      run: () => void onToggleSubagents(model),
    },
    ...(model.catalog === null || model.catalog === undefined
      ? []
      : [
          {
            label: '更新目录快照',
            disabled: locked,
            run: () => void onApplyCatalogUpdate(model),
          },
        ]),
    'separator',
    { label: '删除这个模型', disabled: locked, run: () => onDeleteRequest(model, trigger) },
  ];

  return (
    <article className="model-card" data-default={isDefault || undefined}>
      <header className="model-card-heading">
        <div className="model-card-title">
          <strong>{model.display_name}</strong>
          <small className="model-provider-name">{model.provider_model_name}</small>
        </div>
        <div className="model-card-badges">
          {isDefault ? <span className="status-default">默认</span> : null}
          <span className={profileStatusClass(model)}>{profileStatusLabel(model)}</span>
        </div>
      </header>
      <dl className="model-card-facts">
        <div>
          <dt>可用上下文</dt>
          <dd title={`${formatTokens(model.context.runtime_effective_window_tokens)} tokens`}>
            {compactTokens(model.context.runtime_effective_window_tokens)}
          </dd>
        </div>
        <div>
          <dt>目录标称</dt>
          <dd>{formatOptionalTokens(model.context.catalog_window_tokens)}</dd>
        </div>
        <div>
          <dt>子 Agent</dt>
          <dd>{model.available_for_subagents ? '可用' : '关闭'}</dd>
        </div>
      </dl>
      <footer className="model-card-actions">
        <span className="disabled-action-wrapper" tabIndex={prerequisite === undefined ? -1 : 0}>
          <button
            className="button is-small"
            disabled={prerequisite !== undefined || testing}
            onClick={() => void operation.start()}
            type="button"
          >
            {testing ? '测试中…' : '测试可用性'}
          </button>
          {prerequisite === undefined ? null : <span role="tooltip">{prerequisite}</span>}
        </span>
        <button
          className="button is-small"
          disabled={locked || isDefault}
          onClick={() => void onSetDefault(model)}
          type="button"
        >
          {isDefault ? '当前默认' : '设为默认'}
        </button>
        <span className="model-card-spacer" />
        <button
          className="button is-small is-ghost"
          disabled={locked}
          onClick={() => onEdit(model)}
          type="button"
        >
          编辑
        </button>
        <button
          aria-label={`${model.display_name} 的更多操作`}
          className="icon-button model-card-more"
          onClick={(event) =>
            menu.open(event, `${model.display_name} 的更多操作`, more(event.currentTarget))
          }
          title="更多操作"
          type="button"
        >
          <AppIcon name="more" size={16} />
        </button>
      </footer>
      {menu.menu}
      {operation.pending ? (
        <AvailabilitySkeleton />
      ) : operation.accepted === undefined ? null : (
        <RequestedModelAvailability
          key={operation.accepted.model_availability_test_id}
          accepted={operation.accepted}
          onNotify={onNotify}
          onTerminal={operation.complete}
        />
      )}
    </article>
  );
}

const profileStatusLabel = (profile: ModelProfileConfig): string => {
  if (!profile.enabled) return '已停用';
  if (
    profile.request_defaults.max_output_tokens === null ||
    profile.request_defaults.max_output_tokens === undefined
  ) {
    if (
      profile.catalog?.snapshot.output_limit_tokens === null ||
      profile.catalog?.snapshot.output_limit_tokens === undefined
    ) {
      return '需配置输出上限';
    }
  }
  return '已配置';
};

const profileStatusClass = (profile: ModelProfileConfig): string =>
  profileStatusLabel(profile) === '已配置' ? 'status-ready' : 'status-warning';

const availabilityPrerequisite = (
  endpoint: ModelEndpointConfig,
  credentialState: EndpointCredentialState | undefined,
): string | undefined => {
  if (endpoint.auth_scheme === 'none') return undefined;
  if (credentialState === 'configured') return undefined;
  return '先输入并保存配置后再进行测试';
};

const formatTokens = (value: number): string => new Intl.NumberFormat('zh-CN').format(value);
/** "100万 tokens": a window as people read it; the exact number is its title. */
const compactTokens = (value: number): string =>
  `${new Intl.NumberFormat('zh-CN', { notation: 'compact', maximumFractionDigits: 1 }).format(value)} tokens`;
const formatOptionalTokens = (value: number | null | undefined): string =>
  value === null || value === undefined ? '未声明' : compactTokens(value);
