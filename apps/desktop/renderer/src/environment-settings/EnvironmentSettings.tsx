import { useState } from 'react';
import type { BackgroundMotion, DefaultTerminal } from '@aura/contracts/generated';
import { GitRuntimePanel } from './GitRuntimePanel';
import { ThemeChoice } from './ThemeChoice';
import { BUNDLED_POWERSHELL } from './bundled-shells';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import { useUiSettings } from '../ui-settings-store';
import './environment-settings.css';

const TERMINALS: [DefaultTerminal, string][] = [
  ['power_shell', 'PowerShell 7'],
  ['cmd', 'CMD'],
];

const MOTIONS: [BackgroundMotion, string][] = [
  ['system', '跟随系统'],
  ['on', '开启'],
  ['off', '关闭'],
];

/** Settings → 环境: the local Git and the shells Aura runs, in one place.
 * The terminal a terminal tab opens, the window's theme, whether the sea behind an empty
 * conversation moves and the session's debugging information are chosen here. */
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
      <section aria-label="外观" className="settings-surface environment-theme">
        <h3>外观</h3>
        <ThemeChoice
          chosen={ui.settings.plugin_theme}
          disabled={!ready}
          onChoose={(name) => void save({ plugin_theme: name })}
        />
        <div className="environment-choice">
          <span id="environment-background-motion">背景动画</span>
          <div role="radiogroup" aria-labelledby="environment-background-motion">
            {MOTIONS.map(([value, label]) => (
              <label key={value}>
                <input
                  type="radio"
                  name="background-motion"
                  checked={(ui.settings.background_motion ?? 'system') === value}
                  disabled={!ready}
                  onChange={() => void save({ background_motion: value })}
                />
                {label}
              </label>
            ))}
          </div>
        </div>
      </section>
      <section aria-label="调试" className="settings-surface environment-debug">
        <label className="environment-toggle">
          <ToggleSwitch
            checked={ui.snapshot?.settings.show_debug_information}
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
