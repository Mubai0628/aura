import type { ReasoningEffort } from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';
import type { KeyboardEvent as ReactKeyboardEvent } from 'react';
import { AppIcon } from './app-icons';
import { useDismissablePopover } from './useDismissablePopover';
import { userMessage } from './agent-session/user-message';
import './styles/model-picker.css';

export interface ModelChoice {
  id: string;
  label: string;
  provider: string;
  detail?: string | undefined;
  disabledReason?: string;
  /** The thinking levels the model offers and the one its profile names. */
  reasoning?: { levels: ReasoningEffort[]; level: ReasoningEffort | undefined } | undefined;
}
export interface ModelPickerState {
  choices: ModelChoice[];
  selectedId?: string | undefined;
  loading: boolean;
  error?: string | undefined;
}

interface ProviderGroup {
  name: string;
  choices: ModelChoice[];
}

/** The providers in the order their models come, each with its models. */
function providerGroups(choices: ModelChoice[]): ProviderGroup[] {
  const groups: ProviderGroup[] = [];
  for (const choice of choices) {
    const group = groups.find((item) => item.name === choice.provider);
    if (group) group.choices.push(choice);
    else groups.push({ name: choice.provider, choices: [choice] });
  }
  return groups;
}

/**
 * The menu lists the providers; pointing at
 * one, clicking it or pressing → opens its models beside it. Both lists keep within a height and
 * scroll, so the menu never leaves the window. A single provider lists its models at once.
 */
export function ModelPicker({
  state,
  onSelect,
  onManageSettings,
  openSignal = 0,
}: {
  state: ModelPickerState;
  onSelect: (id: string) => Promise<void>;
  onManageSettings: () => void;
  /** Opens the list whenever it increases (the `/model` command). */
  openSignal?: number;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useDismissablePopover(ref);
  useEffect(() => {
    if (openSignal > 0) setOpen(true);
  }, [openSignal, setOpen]);
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  const [shown, setShown] = useState<string>();
  // A provider opened from the keyboard gives its first model the focus.
  const [byKey, setByKey] = useState(false);
  const selected = state.choices.find((choice) => choice.id === state.selectedId);
  const label = pending
    ? '正在切换…'
    : (selected?.label ?? (state.loading ? '正在加载模型…' : '选择模型'));
  const groups = providerGroups(state.choices);
  const cascade = groups.length > 1;
  const opened = groups.find((group) => group.name === (shown ?? selected?.provider));
  async function select(id: string) {
    if (pending) return;
    setPending(id);
    setError(undefined);
    try {
      await onSelect(id);
      setOpen(false);
    } catch (reason) {
      setError(userMessage(reason, '模型切换未完成，请刷新后重试。'));
    } finally {
      setPending(undefined);
    }
  }
  const toggle = () => {
    // Each opening starts at the provider of the model in use.
    if (!open) setShown(undefined);
    setOpen(!open);
  };
  const models = (group: ProviderGroup, focusFirst = false) => (
    <ModelList
      choices={group.choices}
      focusFirst={focusFirst}
      label={cascade ? `${group.name} 的模型` : undefined}
      pending={pending}
      selectedId={state.selectedId}
      onSelect={(id) => void select(id)}
    />
  );
  return (
    <div className="composer-menu-anchor model-menu-anchor" ref={ref}>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        className="composer-select model-select"
        data-adaptive
        onClick={toggle}
        title={`模型：${label}`}
      >
        <AppIcon name="model" size={16} />
        <span data-adaptive-label>{label}</span>
        <AppIcon className="adaptive-chevron" name="chevron-down" size={10} />
      </button>
      {open && (
        <div
          aria-label="快速选择模型"
          className={`composer-popover model-popover${cascade ? ' is-cascade' : ''}`}
          role="menu"
        >
          {state.loading && !state.choices.length ? (
            <span className="composer-menu-empty" role="status">
              正在读取模型配置…
            </span>
          ) : state.error ? (
            <span className="composer-menu-empty" role="alert">
              {userMessage(state.error, '模型配置读取失败，请重新打开或检查连接。')}
            </span>
          ) : !groups.length ? (
            <span className="composer-menu-empty">尚未添加模型，请前往设置添加。</span>
          ) : null}
          {cascade ? (
            <ProviderList
              groups={groups}
              opened={opened?.name}
              selectedProvider={selected?.provider}
              onOpen={(name, key) => {
                setShown(name);
                setByKey(key);
              }}
            />
          ) : groups[0] ? (
            models(groups[0])
          ) : null}
          {error && <p role="alert">{error}</p>}
          <button
            type="button"
            role="menuitem"
            className="composer-menu-footer"
            onClick={() => {
              setOpen(false);
              onManageSettings();
            }}
          >
            管理模型 <AppIcon name="chevron-right" size={14} />
          </button>
          {cascade && opened ? (
            <div className="composer-popover model-submenu">
              {models(opened, shown === opened.name && byKey)}
            </div>
          ) : null}
        </div>
      )}
    </div>
  );
}

function ProviderList({
  groups,
  opened,
  selectedProvider,
  onOpen,
}: {
  groups: ProviderGroup[];
  opened: string | undefined;
  selectedProvider: string | undefined;
  onOpen: (name: string, byKey: boolean) => void;
}) {
  const open = onOpen;
  const onKeyDown = (event: ReactKeyboardEvent<HTMLButtonElement>, name: string) => {
    if (event.key === 'ArrowRight' || event.key === 'Enter' || event.key === ' ') {
      event.preventDefault();
      open(name, true);
    } else if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
      event.preventDefault();
      const items = [
        ...(event.currentTarget.parentElement?.querySelectorAll('button') ?? []),
      ] as HTMLButtonElement[];
      const at = items.indexOf(event.currentTarget);
      items[(at + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
    }
  };
  return (
    <div aria-label="提供商" className="model-provider-list model-menu-scroll" role="group">
      {groups.map((group) => (
        <button
          aria-expanded={opened === group.name}
          aria-haspopup="menu"
          className="model-provider"
          data-current={selectedProvider === group.name ? 'true' : undefined}
          key={group.name}
          onClick={() => open(group.name, false)}
          onFocus={() => open(group.name, false)}
          onKeyDown={(event) => onKeyDown(event, group.name)}
          onMouseEnter={() => open(group.name, false)}
          role="menuitem"
          type="button"
        >
          <span>{group.name}</span>
          <small>{group.choices.length}</small>
          <AppIcon name="chevron-right" size={12} />
        </button>
      ))}
    </div>
  );
}

function ModelList({
  choices,
  label,
  pending,
  selectedId,
  focusFirst,
  onSelect,
}: {
  choices: ModelChoice[];
  label: string | undefined;
  pending: string | undefined;
  selectedId: string | undefined;
  focusFirst: boolean;
  onSelect: (id: string) => void;
}) {
  const list = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!focusFirst) return;
    const checked = list.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]');
    (checked ?? list.current?.querySelector<HTMLButtonElement>('button'))?.focus();
  }, [focusFirst, label]);
  const onKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    if (event.key !== 'ArrowDown' && event.key !== 'ArrowUp') return;
    event.preventDefault();
    const items = [...(list.current?.querySelectorAll('button') ?? [])] as HTMLButtonElement[];
    const at = items.indexOf(document.activeElement as HTMLButtonElement);
    items[(at + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length]?.focus();
  };
  return (
    <div
      aria-label={label}
      className="model-menu-group model-menu-scroll"
      onKeyDown={onKeyDown}
      ref={list}
      role={label ? 'menu' : 'group'}
    >
      {choices.map((choice) => (
        <button
          type="button"
          role="menuitemradio"
          key={choice.id}
          aria-checked={choice.id === selectedId}
          disabled={!!pending || !!choice.disabledReason}
          title={choice.disabledReason ?? choice.detail}
          onClick={() => onSelect(choice.id)}
        >
          <span>
            {choice.label}
            {choice.detail ? <small className="model-choice-detail">{choice.detail}</small> : null}
          </span>
          {choice.id === selectedId && <AppIcon name="check" size={14} />}
        </button>
      ))}
    </div>
  );
}
