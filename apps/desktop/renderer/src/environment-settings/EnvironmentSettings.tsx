import { useState } from 'react';
import type { DefaultTerminal } from '@aura/contracts/generated';
import { GitRuntimePanel } from './GitRuntimePanel';
import { BUNDLED_POWERSHELL } from './bundled-shells';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { useUiSettings } from '../ui-settings-store';
import './environment-settings.css';

const TERMINALS: [DefaultTerminal, string][] = [
  ['power_shell', 'PowerShell 7'],
  ['cmd', 'CMD'],
];

/** Settings → 环境: the local Git and the shells Aura runs, in one place.
 * The terminal a terminal tab opens, and the session's debugging
 * information, are chosen here. */
export function EnvironmentSettings() {
  const ui = useUiSettings();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();
  const ready = ui.snapshot?.revision != null && !saving;
  const save = async (change: Partial<typeof ui.settings>) => {
    setSaving(true);
    setError(await ui.save({ ...ui.settings, ...change }));
    setSaving(false);
  };
  return (
    <div className="settings-page environment-settings">
      <div className="settings-content-heading">
        <div>
          <h2>环境</h2>
        </div>
      </div>
      <GitRuntimePanel />
      <section aria-label="终端与命令" className="settings-surface environment-shells">
        <h3>终端与命令</h3>
        <div className="environment-choice">
          <span id="environment-default-terminal">默认终端</span>
          <div role="radiogroup" aria-labelledby="environment-default-terminal">
            {TERMINALS.map(([value, label]) => (
              <label key={value}>
                <input
                  type="radio"
                  name="default-terminal"
                  checked={ui.settings.default_terminal === value}
                  disabled={!ready}
                  onChange={() => void save({ default_terminal: value })}
                />
                {label}
              </label>
            ))}
          </div>
        </div>
        <dl>
          <dt>{`PowerShell ${BUNDLED_POWERSHELL.version}`}</dt>
          <dd>随 Aura 安装</dd>
          <dd className="environment-shell-detail">
            {BUNDLED_POWERSHELL.architecture} · PowerShell 工具与终端使用
          </dd>
          <dt>CMD</dt>
          <dd>Windows 自带</dd>
          <dd className="environment-shell-detail">CMD 工具与终端使用</dd>
        </dl>
      </section>
      <section aria-label="调试" className="settings-surface environment-debug">
        <label className="environment-toggle">
          <ToggleSwitch
            checked={ui.settings.show_debug_information}
            disabled={!ready}
            label="显示调试信息"
            onCheckedChange={(checked) => void save({ show_debug_information: checked })}
          />
          <span>
            显示调试信息
            <small>会话顶部的运行能力与工具执行记录</small>
          </span>
        </label>
      </section>
      {error || ui.snapshot?.issue || (ui.error && !ui.snapshot) ? (
        <p className="settings-status-line" role="alert">
          {error ?? ui.snapshot?.issue ?? ui.error}
        </p>
      ) : null}
    </div>
  );
}
