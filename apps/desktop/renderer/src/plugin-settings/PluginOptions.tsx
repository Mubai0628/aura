import { useRef, useState } from 'react';
import type { PluginOption, PluginView } from '@aura/contracts/generated';
import { McpSecretInput, type McpSecretInputHandle } from '../mcp-settings/McpSecretInput';

const titled = (option: PluginOption) =>
  option.required ? `${option.title}（必填）` : option.title;

/**
 * The plugin's own settings: the text ones saved together, each secret one set on its own
 * channel and shown only as set. A typed secret never enters React state.
 */
export function PluginOptions({
  plugin,
  busy,
  onSaveText,
  onSetSecret,
  onClearSecret,
}: {
  plugin: PluginView;
  busy: boolean;
  onSaveText: (text: Record<string, string>) => void;
  onSetSecret: (option: PluginOption, bytes: Uint8Array) => void;
  onClearSecret: (option: PluginOption) => void;
}) {
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const secrets = useRef(new Map<string, McpSecretInputHandle | null>());
  const options = plugin.options ?? [];
  if (!options.length) return null;
  const changed: Record<string, string> = {};
  for (const { option, text } of options) {
    const draft = drafts[option.key];
    if (option.kind === 'text' && draft !== undefined && draft.trim() !== (text ?? ''))
      changed[option.key] = draft.trim();
  }
  const texts = options.filter((value) => value.option.kind === 'text');
  return (
    <section className="plugin-options" aria-label="设置">
      <strong>设置</strong>
      {options.map(({ option, text, set }) =>
        option.kind === 'text' ? (
          <label key={option.key} className="plugin-option">
            {titled(option)}
            <input
              type="text"
              value={drafts[option.key] ?? text ?? ''}
              maxLength={2000}
              disabled={busy}
              onChange={(event) => {
                const value = event.currentTarget.value;
                setDrafts((known) => ({ ...known, [option.key]: value }));
              }}
            />
          </label>
        ) : (
          <div key={option.key} className="plugin-option is-secret">
            <McpSecretInput
              ref={(handle) => {
                secrets.current.set(option.key, handle);
              }}
              label={titled(option)}
              saved={set === true}
              disabled={busy}
            />
            <button
              type="button"
              className="button is-small"
              aria-label={`保存 ${option.title}`}
              disabled={busy}
              onClick={() => {
                const bytes = secrets.current.get(option.key)?.takeBytes();
                if (bytes) onSetSecret(option, bytes);
              }}
            >
              保存
            </button>
            {set ? (
              <button
                type="button"
                className="button is-ghost is-small"
                aria-label={`清除 ${option.title}`}
                disabled={busy}
                onClick={() => onClearSecret(option)}
              >
                清除
              </button>
            ) : null}
          </div>
        ),
      )}
      {texts.length ? (
        <div className="settings-card-actions">
          <button
            type="button"
            className="button is-small"
            disabled={busy || Object.keys(changed).length === 0}
            onClick={() => onSaveText(changed)}
          >
            保存设置
          </button>
        </div>
      ) : null}
    </section>
  );
}
