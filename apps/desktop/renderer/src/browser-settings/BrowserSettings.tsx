import { useEffect, useId, useState, type FormEvent } from 'react';
import type {
  BrowserSettings as Settings,
  BrowserSettingsSnapshot,
} from '@aura/contracts/generated';

import { AppIcon } from '../app-icons';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { userMessage } from '../agent-session/user-message';
import { browserApi } from '../browser/browser-store';
import { ToggleSwitch } from '../ui/ToggleSwitch';
import './browser-settings.css';

type SiteField = 'allowed_sites' | 'blocked_sites';
const LISTS: Record<SiteField, string> = {
  allowed_sites: '只读会话直接打开的网站',
  blocked_sites: '阻止的网站',
};

/** The risk, stated plainly where the switch is. */
export const DEBUGGING_RISK =
  '开启后，模型可以在网页里运行它写的脚本，并读取完整的网络请求记录。脚本以网页的身份运行，能读取和改动该网站的数据，包括你在内置浏览器里的登录状态。在公网网页上运行脚本，每次仍会先问你。';

/** Core names the list and the site; the page says it in its own words. */
const readable = (message: string) =>
  message.replace(
    /^.*?(allowed_sites|blocked_sites): /u,
    (_all, field: SiteField) => `${LISTS[field]}：`,
  );

function SiteList({
  field,
  sites,
  disabled,
  onChange,
}: {
  field: SiteField;
  sites: readonly string[];
  disabled: boolean;
  onChange: (sites: string[]) => Promise<boolean>;
}) {
  const [draft, setDraft] = useState('');
  const headingId = useId();
  const inputId = useId();
  const add = (event: FormEvent) => {
    event.preventDefault();
    const site = draft.trim();
    if (site === '') return;
    void onChange([...sites, site]).then((saved) => {
      if (saved) setDraft('');
    });
  };
  return (
    <section aria-labelledby={headingId} className="browser-site-list">
      <h4 id={headingId}>{LISTS[field]}</h4>
      {sites.length > 0 ? (
        <ul>
          {sites.map((site) => (
            <li key={site}>
              <code>{site}</code>
              <button
                aria-label={`移除 ${site}`}
                className="icon-button"
                disabled={disabled}
                onClick={() => void onChange(sites.filter((item) => item !== site))}
                type="button"
              >
                <AppIcon name="close" size={13} />
              </button>
            </li>
          ))}
        </ul>
      ) : null}
      <form onSubmit={add}>
        <label className="sr-only" htmlFor={inputId}>
          {`添加到${LISTS[field]}`}
        </label>
        <input
          autoComplete="off"
          disabled={disabled}
          id={inputId}
          onChange={(event) => setDraft(event.target.value)}
          spellCheck={false}
          value={draft}
        />
        <button
          className="secondary-button"
          disabled={disabled || draft.trim() === ''}
          type="submit"
        >
          添加
        </button>
      </form>
    </section>
  );
}

/**
 * 设置 → 网络与浏览器: the browser switch, the sites a ReadOnly session opens without asking,
 * the sites never loaded, and the advanced debugging switch (off unless the user turns it on).
 */
export function BrowserSettings({ coreReady }: { coreReady: boolean }) {
  const [snapshot, setSnapshot] = useState<BrowserSettingsSnapshot>();
  const [error, setError] = useState<string>();
  const [saving, setSaving] = useState(false);
  const [confirming, setConfirming] = useState<'debugging' | 'clear'>();
  const debugTitle = useId();
  const clearTitle = useId();

  useEffect(() => {
    const api = browserApi();
    if (!coreReady || api === undefined) return;
    let live = true;
    void api.getSettings().then((result) => {
      if (!live) return;
      if (result.ok) setSnapshot(result.value);
      else setError(userMessage(result.error, '浏览器设置读取失败，请重试。'));
    });
    return () => {
      live = false;
    };
  }, [coreReady]);

  const settings = snapshot?.settings;
  const ready = snapshot?.revision != null && !saving;
  const save = async (change: Partial<Settings>): Promise<boolean> => {
    const api = browserApi();
    if (api === undefined || settings === undefined || snapshot?.revision == null) return false;
    setSaving(true);
    const result = await api.updateSettings({
      expected_revision: snapshot.revision,
      settings: { ...settings, ...change },
    });
    setSaving(false);
    if (result.ok) {
      setSnapshot(result.value);
      setError(undefined);
      return true;
    }
    setError(readable(userMessage(result.error, '浏览器设置没有保存，请重试。')));
    return false;
  };

  return (
    <div className="settings-page browser-settings">
      <div className="settings-content-heading">
        <div>
          <h2>网络与浏览器</h2>
        </div>
      </div>
      <section aria-label="内置浏览器" className="settings-surface browser-settings-row">
        <label className="browser-settings-toggle">
          <ToggleSwitch
            checked={settings?.enabled ?? false}
            disabled={!ready}
            label="启用内置浏览器"
            onCheckedChange={(checked) => void save({ enabled: checked })}
          />
          <span>启用内置浏览器</span>
        </label>
        <button
          className="secondary-button"
          disabled={!ready}
          onClick={() => setConfirming('clear')}
          type="button"
        >
          清除浏览数据
        </button>
      </section>
      <section aria-label="网站" className="settings-surface browser-settings-sites">
        {(['allowed_sites', 'blocked_sites'] as const).map((field) => (
          <SiteList
            disabled={!ready}
            field={field}
            key={field}
            onChange={(sites) => save({ [field]: sites })}
            sites={settings?.[field] ?? []}
          />
        ))}
      </section>
      <section aria-label="浏览器调试" className="settings-surface browser-settings-debug">
        <label className="browser-settings-toggle">
          <ToggleSwitch
            checked={settings?.debugging ?? false}
            disabled={!ready}
            label="允许模型调试网页"
            onCheckedChange={(checked) =>
              checked ? setConfirming('debugging') : void save({ debugging: false })
            }
          />
          <span>
            允许模型调试网页
            <small>{DEBUGGING_RISK}</small>
          </span>
        </label>
      </section>
      {error !== undefined || snapshot?.issue ? (
        <p className="settings-status-line" role="alert">
          {error ?? snapshot?.issue}
        </p>
      ) : null}
      {confirming === 'debugging' ? (
        <ConfirmationDialog
          busy={saving}
          confirmLabel="允许"
          description={DEBUGGING_RISK}
          onCancel={() => setConfirming(undefined)}
          onConfirm={() => {
            setConfirming(undefined);
            void save({ debugging: true });
          }}
          title="允许模型调试网页？"
          titleId={debugTitle}
        />
      ) : null}
      {confirming === 'clear' ? (
        <ConfirmationDialog
          busy={false}
          confirmLabel="清除"
          description="内置浏览器里登录的网站都会退出，缓存也会清空。"
          onCancel={() => setConfirming(undefined)}
          onConfirm={() => {
            setConfirming(undefined);
            void browserApi()
              ?.command({ kind: 'clear-data' })
              .then((reply) => {
                if (!reply.ok) setError(reply.message);
              });
          }}
          title="清除浏览数据？"
          titleId={clearTitle}
        />
      ) : null}
    </div>
  );
}
