import type {
  PermissionMode,
  PermissionModeTransition,
  SessionModelSelectorProjection,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

import { ModelPicker } from './ModelPicker';
import { SessionReasoningPicker } from './ReasoningPicker';
import { userMessage } from './agent-session/user-message';
import { AppIcon } from './app-icons';
import { useDismissablePopover } from './useDismissablePopover';

const PERMISSION_LABELS: Record<PermissionMode, string> = {
  full_access: '完全访问',
  read_only: '只读',
  workspace_read_write: '工作区读写',
};

const PERMISSION_MODES = Object.keys(PERMISSION_LABELS) as PermissionMode[];

interface PermissionQuickSelectorProps {
  /** Opens the menu whenever it increases (the `/permission` command). */
  openSignal?: number;
  demoMode: boolean;
  mode: PermissionMode | undefined;
  transition?: PermissionModeTransition | undefined;
  onManageSettings: () => void;
  onSelect: (mode: PermissionMode) => Promise<void>;
}

export function PermissionQuickSelector({
  openSignal = 0,
  demoMode,
  mode,
  transition,
  onManageSettings,
  onSelect,
}: PermissionQuickSelectorProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [menuOpen, setMenuOpen] = useDismissablePopover(containerRef);
  useEffect(() => {
    if (openSignal > 0) setMenuOpen(true);
  }, [openSignal, setMenuOpen]);
  const [demoValue, setDemoValue] = useState<PermissionMode>('workspace_read_write');
  const [pending, setPending] = useState(false);
  const selected = mode ?? (demoMode ? demoValue : undefined);
  const label = selected === undefined ? '权限尚未就绪' : PERMISSION_LABELS[selected];
  const status =
    transition === 'stopping'
      ? '停止中…'
      : transition === 'stop_unconfirmed'
        ? '待确认停止'
        : undefined;

  const selectMode = async (next: PermissionMode) => {
    // One change at a time: the menu stays open while Core answers.
    if (pending) return;
    if (demoMode) {
      setDemoValue(next);
    } else {
      setPending(true);
      try {
        await onSelect(next);
      } finally {
        setPending(false);
      }
    }
    setMenuOpen(false);
  };

  return (
    <div className="composer-menu-anchor" ref={containerRef}>
      <button
        aria-expanded={menuOpen}
        aria-haspopup="menu"
        className="composer-select"
        data-adaptive
        disabled={pending || selected === undefined}
        onClick={() => setMenuOpen(!menuOpen)}
        title={`权限：${label}${status ? ` · ${status}` : ''}`}
        type="button"
      >
        <AppIcon name="shield" size={16} />
        <span data-adaptive-label>{label}</span>
        {status ? (
          <span data-adaptive-label role="status">
            {status}
          </span>
        ) : null}
        <AppIcon className="adaptive-chevron" name="chevron-down" size={10} />
      </button>
      {menuOpen ? (
        <div aria-label="快速选择权限" className="composer-popover" role="menu">
          <p>{demoMode ? '演示选择 · 不写入 Core' : '当前会话权限'}</p>
          {transition === 'stop_unconfirmed' ? (
            <p role="alert">历史停止状态尚未确认，请检查执行记录；需要停止的程序请明确停止。</p>
          ) : null}
          {PERMISSION_MODES.map((value) => (
            <button
              aria-checked={selected === value}
              disabled={pending}
              key={value}
              onClick={() => void selectMode(value)}
              role="menuitemradio"
              type="button"
            >
              <span>{PERMISSION_LABELS[value]}</span>
              {selected === value ? <AppIcon name="check" size={14} /> : null}
            </button>
          ))}
          <button
            className="composer-menu-footer"
            onClick={() => {
              setMenuOpen(false);
              onManageSettings();
            }}
            role="menuitem"
            type="button"
          >
            会话授权管理
            <AppIcon name="chevron-right" size={14} />
          </button>
        </div>
      ) : null}
    </div>
  );
}

export function ModelQuickSelector({
  onSelect,
  onManageSettings,
  projection,
  error,
  openSignal,
}: {
  openSignal?: number;
  onManageSettings: () => void;
  onSelect: (modelProfileId: string) => Promise<void>;
  projection: SessionModelSelectorProjection | undefined;
  error?: string | undefined;
}) {
  const choices = (projection?.models ?? []).map((model) => {
    const endpoint = projection?.endpoints.find((item) => item.endpoint_id === model.endpoint_id);
    const provider = projection?.providers.find(
      (item) => item.provider_id === endpoint?.provider_id,
    );
    return {
      id: model.model_profile_id,
      label: model.display_name,
      provider: provider?.display_name ?? '模型',
      detail: endpoint?.display_name,
      reasoning: {
        levels: model.reasoning_efforts ?? [],
        level: model.reasoning_effort ?? undefined,
      },
      ...(!model.execution_readiness.is_executable
        ? {
            disabledReason: userMessage(
              model.execution_readiness.issue_code,
              '请先在模型设置中补全配置。',
            ),
          }
        : {}),
    };
  });
  const selected = choices.find((choice) => choice.id === projection?.selected_model_profile_id);
  return (
    <>
      <ModelPicker
        state={{
          choices,
          selectedId: projection?.selected_model_profile_id ?? undefined,
          loading: projection === undefined && !error,
          error,
        }}
        onSelect={onSelect}
        onManageSettings={onManageSettings}
        {...(openSignal === undefined ? {} : { openSignal })}
      />
      {selected ? (
        <SessionReasoningPicker
          fallback={selected.reasoning?.level}
          levels={selected.reasoning?.levels ?? []}
          modelProfileId={selected.id}
          sessionId={projection?.session_id ?? ''}
        />
      ) : null}
    </>
  );
}
