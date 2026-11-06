import type { HookEntryView, HookPreview } from '@aura/contracts/generated';
import { HookInvocations } from './HookInvocations';
import { HookPreviewPanel } from './HookPreviewPanel';
import {
  hookEvents,
  hookKindLabel,
  hookName,
  hookReview,
  hookStateLabel,
  hookStateTone,
} from './hook-labels';

export type HookChange = 'enable' | 'disable' | 'untrust' | 'remove';

/** A Hook's place in the order, name, version, kind, plugin and state. */
export function HookHeading({
  entry,
  order,
  named = true,
}: {
  entry: HookEntryView;
  order?: number | undefined;
  named?: boolean;
}) {
  const { definition } = entry;
  return (
    <>
      {order === undefined ? null : <span className="hook-row-order">{order}</span>}
      {named ? <strong>{hookName(entry)}</strong> : null}
      {definition ? <span className="hook-row-version">{definition.version}</span> : null}
      {definition ? <span className="hook-row-kind">{hookKindLabel(definition)}</span> : null}
      {entry.plugin ? <span className="settings-tag">插件 {entry.plugin}</span> : null}
      <span className="status-pill hook-row-state" data-tone={hookStateTone(entry.state)}>
        {hookStateLabel(entry.state)}
      </span>
    </>
  );
}

/** One Hook in its dialog: what it is, where it is, its review, its recent calls and the user's
 * choices. */
export function HookDetails({
  entry,
  locked,
  preview,
  removing,
  history,
  onPreview,
  onClosePreview,
  onTrust,
  onChange,
  onToggleHistory,
  onAskRemove,
  onCancelRemove,
}: {
  entry: HookEntryView;
  locked: boolean;
  preview: HookPreview | undefined;
  removing: boolean;
  history: boolean;
  onPreview: () => void;
  onClosePreview: () => void;
  onTrust: (preview: HookPreview) => void;
  onChange: (action: HookChange) => void;
  onToggleHistory: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
}) {
  const name = hookName(entry);
  const events = hookEvents(entry);
  // A plugin's Hook is trusted and removed with its plugin, and runs after the user's own.
  const plugin = entry.plugin ?? undefined;
  return (
    <div className="hook-details">
      {entry.definition?.description ? <p>{entry.definition.description}</p> : null}
      {events ? <p className="hook-row-events">{events}</p> : null}
      <p className="hook-row-location">{entry.path}</p>
      {entry.problems.length ? (
        <p className="settings-inline-alert" role="alert">
          {entry.problems.join('；')}
        </p>
      ) : null}
      <div className="settings-card-actions">
        {plugin ? null : (
          // The button that opens the preview also closes it.
          <button
            type="button"
            className={
              !preview && hookReview(entry) ? 'button is-primary is-small' : 'button is-small'
            }
            disabled={locked}
            aria-expanded={preview !== undefined}
            onClick={preview ? onClosePreview : onPreview}
          >
            {preview ? '收起' : (hookReview(entry) ?? '预览')}
          </button>
        )}
        {entry.state === 'ready' || entry.state === 'disabled' ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange(entry.enabled ? 'disable' : 'enable')}
          >
            {entry.enabled ? '停用' : '启用'}
          </button>
        ) : null}
        {entry.trusted_fingerprint && !plugin ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange('untrust')}
          >
            撤销信任
          </button>
        ) : null}
        <button
          type="button"
          className="button is-ghost is-small"
          aria-expanded={history}
          onClick={onToggleHistory}
        >
          {history ? '收起调用记录' : '最近调用'}
        </button>
        {plugin ? null : (
          <button
            type="button"
            className="button is-danger is-small hook-remove"
            disabled={locked}
            onClick={onAskRemove}
          >
            移除
          </button>
        )}
      </div>
      {removing ? (
        <div role="alertdialog" aria-label={`移除 ${name}`} className="settings-panel is-danger">
          <p>
            只从列表中移除 {name}，不会删除它的文件夹（{entry.path}）。之后可以重新添加。
          </p>
          <div className="settings-card-actions">
            <button
              type="button"
              className="button is-danger is-solid is-small"
              onClick={() => onChange('remove')}
            >
              移除
            </button>
            <button type="button" className="button is-small" onClick={onCancelRemove}>
              取消
            </button>
          </div>
        </div>
      ) : null}
      {preview ? (
        <HookPreviewPanel preview={preview} busy={locked} onTrust={() => onTrust(preview)} />
      ) : null}
      {history ? <HookInvocations path={entry.path} name={name} /> : null}
    </div>
  );
}
