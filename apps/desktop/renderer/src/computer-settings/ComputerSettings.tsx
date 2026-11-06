import { useCallback, useEffect, useId, useState } from 'react';
import type {
  ComputerControlSettings as Settings,
  ComputerControlSettingsSnapshot,
  ComputerMode,
} from '@aura/contracts/generated';

import type { ResultEnvelope } from '../../../shared/aura-api';
import { userMessage } from '../agent-session/user-message';
import { useDraftModelChoices } from '../agent-session/useDraftModelChoices';
import { computerControl } from '../computer/bridge';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { AddedPrograms, AllowedPrograms } from './ProgramLists';
import { StopKeys } from './StopKeys';
import './computer-settings.css';

/** Where a new turn operates programs: on the user's desktop, or in a window of its own. */
const MODES: readonly { value: ComputerMode; label: string }[] = [
  { value: 'foreground', label: '前台' },
  { value: 'child_session', label: '小窗' },
];

/** The longest edges a screenshot may be given, in pixels; Core takes 640 to 3840. */
const LONG_EDGES = [1024, 1568, 2048, 2560, 3840] as const;

const UNREADABLE = '读不到电脑操控的设置，请重试。';
const NOT_SAVED = '电脑操控的设置没有保存，请重试。';

type Switch = 'enabled' | 'paint_others' | 'clipboard' | 'system_keys';

const SWITCHES: readonly { field: Exclude<Switch, 'enabled'>; label: string; group: string }[] = [
  { field: 'paint_others', label: '截图时涂掉其他程序', group: '截图' },
  { field: 'clipboard', label: '允许使用剪贴板', group: '输入' },
  { field: 'system_keys', label: '允许系统组合键', group: '输入' },
];

/**
 * 设置 → 电脑操控: the switch, where new turns operate programs and the keys that take over,
 * the programs every conversation operates without asking, the programs added by path, what a
 * screenshot shows, what input may use, and which models get the tools.
 */
export function ComputerSettings({ coreReady }: { coreReady: boolean }) {
  const [snapshot, setSnapshot] = useState<ComputerControlSettingsSnapshot>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  // 指定的模型 chosen before any model is ticked: nothing changes until one is.
  const [naming, setNaming] = useState(false);
  const models = useDraftModelChoices();
  const sizeId = useId();
  const modeId = useId();

  const show = useCallback((result: ResultEnvelope<ComputerControlSettingsSnapshot>) => {
    if (result.ok) {
      setSnapshot(result.value);
      setError(undefined);
    } else setError(userMessage(result.error, UNREADABLE));
  }, []);
  const read = useCallback(async () => {
    try {
      const result = await computerControl()?.settings();
      if (result !== undefined) show(result);
    } catch {
      setError(UNREADABLE);
    }
  }, [show]);
  useEffect(() => {
    const api = computerControl();
    if (!coreReady || api === undefined) return;
    let live = true;
    void api.settings().then(
      (result) => {
        if (live) show(result);
      },
      () => {
        if (live) setError(UNREADABLE);
      },
    );
    return () => {
      live = false;
    };
  }, [coreReady, show]);

  const settings = snapshot?.settings;
  const ready = snapshot?.revision != null && !saving;
  const save = async (change: Partial<Settings>): Promise<boolean> => {
    const api = computerControl();
    if (api === undefined || settings === undefined || snapshot?.revision == null) return false;
    setSaving(true);
    try {
      const result = await api.updateSettings({
        expected_revision: snapshot.revision,
        settings: { ...settings, ...change },
      });
      if (result.ok) {
        setSnapshot(result.value);
        setError(undefined);
        return true;
      }
      const message = userMessage(result.error, NOT_SAVED);
      // Changed elsewhere, as by an approval card's 总是允许: the page shows what is saved now,
      // and still says why the change did not go in.
      if (result.error.message.includes('computer_settings.stale')) await read();
      setError(message);
      return false;
    } catch {
      setError(NOT_SAVED);
      return false;
    } finally {
      setSaving(false);
    }
  };

  const named = (settings?.models.length ?? 0) > 0 || naming;
  const sizes =
    settings && !LONG_EDGES.some((edge) => edge === settings.screenshot_long_edge)
      ? [...LONG_EDGES, settings.screenshot_long_edge].sort((a, b) => a - b)
      : LONG_EDGES;
  const toggle = (field: Switch, label: string) => (
    <label className="computer-settings-toggle" key={field}>
      <ToggleSwitch
        checked={settings?.[field]}
        disabled={!ready}
        label={label}
        onCheckedChange={(checked) => void save({ [field]: checked })}
      />
      <span>{label}</span>
    </label>
  );
  return (
    <div className="settings-page computer-settings">
      <div className="settings-content-heading">
        <div>
          <h2>电脑操控</h2>
        </div>
      </div>
      <section aria-label="电脑操控开关" className="settings-surface">
        {toggle('enabled', '电脑操控')}
      </section>
      <section aria-label="操控方式" className="settings-surface computer-settings-group">
        <label className="computer-settings-size" htmlFor={modeId}>
          <span>操控方式</span>
          <select
            disabled={!ready}
            id={modeId}
            onChange={(event) => void save({ mode: event.target.value as ComputerMode })}
            value={settings?.mode ?? 'foreground'}
          >
            {MODES.map((mode) => (
              <option key={mode.value} value={mode.value}>
                {mode.label}
              </option>
            ))}
          </select>
        </label>
        <StopKeys
          key={settings?.pip_stop_keys ?? 'Ctrl+Alt+Q'}
          value={settings?.pip_stop_keys ?? 'Ctrl+Alt+Q'}
          disabled={!ready}
          save={(keys) => save({ pip_stop_keys: keys })}
        />
      </section>
      <section aria-label="程序" className="settings-surface computer-settings-programs">
        <AllowedPrograms
          apps={settings?.always_allowed ?? []}
          disabled={!ready}
          onChange={(apps) => save({ always_allowed: apps })}
        />
        <AddedPrograms
          apps={settings?.added ?? []}
          disabled={!ready}
          onChange={(apps) => save({ added: apps })}
        />
      </section>
      {(['截图', '输入'] as const).map((group) => (
        <section
          aria-label={group}
          className="settings-surface computer-settings-group"
          key={group}
        >
          <h4>{group}</h4>
          {SWITCHES.filter((one) => one.group === group).map((one) => toggle(one.field, one.label))}
          {group === '截图' ? (
            <label className="computer-settings-size" htmlFor={sizeId}>
              <span>截图长边</span>
              <select
                disabled={!ready}
                id={sizeId}
                onChange={(event) =>
                  void save({ screenshot_long_edge: Number(event.target.value) })
                }
                value={settings?.screenshot_long_edge ?? 1568}
              >
                {sizes.map((edge) => (
                  <option key={edge} value={edge}>
                    {`${edge} 像素`}
                  </option>
                ))}
              </select>
            </label>
          ) : null}
        </section>
      ))}
      <section aria-label="可用的模型" className="settings-surface computer-settings-group">
        <h4>可用的模型</h4>
        <label className="computer-settings-choice">
          <input
            checked={!named}
            disabled={!ready}
            name="computer-models"
            onChange={() => {
              setNaming(false);
              if ((settings?.models.length ?? 0) > 0) void save({ models: [] });
            }}
            type="radio"
          />
          <span>能看图的模型</span>
        </label>
        <label className="computer-settings-choice">
          <input
            checked={named}
            disabled={!ready}
            name="computer-models"
            onChange={() => setNaming(true)}
            type="radio"
          />
          <span>指定的模型</span>
        </label>
        {named ? (
          <ul className="computer-settings-models">
            {models.choices.map((model) => {
              const ticked = settings?.models.includes(model.id) ?? false;
              return (
                <li key={model.id}>
                  <label>
                    <input
                      checked={ticked}
                      disabled={!ready}
                      onChange={() => {
                        const current = settings?.models ?? [];
                        const next = ticked
                          ? current.filter((id) => id !== model.id)
                          : [...current, model.id];
                        // None ticked is what 能看图的模型 means, and the page says so.
                        if (next.length === 0) setNaming(false);
                        void save({ models: next });
                      }}
                      type="checkbox"
                    />
                    <span>{model.label}</span>
                    <small>{model.provider}</small>
                  </label>
                </li>
              );
            })}
          </ul>
        ) : null}
      </section>
      {error !== undefined || snapshot?.issue ? (
        <p className="settings-status-line" role="alert">
          {error ?? snapshot?.issue}
          {snapshot === undefined ? (
            <button className="button is-small" onClick={() => void read()} type="button">
              重试
            </button>
          ) : null}
        </p>
      ) : null}
    </div>
  );
}
