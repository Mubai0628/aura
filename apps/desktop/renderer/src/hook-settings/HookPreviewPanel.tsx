import type { HookPreview } from '@aura/contracts/generated';
import {
  folderName,
  hookCommandLabel,
  hookEventLabel,
  hookKindLabel,
  hookToolsLabel,
  shortFingerprint,
} from './hook-labels';

const size = (bytes: number) =>
  bytes < 1024 ? `${bytes} 字节` : `${(bytes / 1024).toFixed(bytes < 10_240 ? 1 : 0)} KB`;

/** Everything the user sees before trusting a Hook folder: what runs, when, and on which files.
 * A listed Hook's preview is closed by the button that opened it; only a folder being
 * added, which has no row yet, gets `onCancel`. */
export function HookPreviewPanel({
  preview,
  busy,
  onTrust,
  onCancel,
}: {
  preview: HookPreview;
  busy: boolean;
  onTrust: () => void;
  onCancel?: (() => void) | undefined;
}) {
  const { definition } = preview;
  const name = definition?.name ?? folderName(preview.path);
  const trustable = definition != null && preview.fingerprint != null && !preview.problems.length;
  const tools = definition ? hookToolsLabel(definition) : undefined;
  return (
    <section className="hook-preview settings-panel is-accent" aria-label={`预览 Hook ${name}`}>
      <h3>
        预览：{name}
        {definition ? <span> {definition.version}</span> : null}
      </h3>
      <p className="hook-row-location">{preview.path}</p>
      {preview.problems.length ? (
        <ul role="alert" className="hook-problems">
          {preview.problems.map((problem) => (
            <li key={problem}>{problem}</li>
          ))}
        </ul>
      ) : null}
      {definition ? (
        <dl className="hook-facts">
          {definition.description ? (
            <>
              <dt>说明</dt>
              <dd>{definition.description}</dd>
            </>
          ) : null}
          <dt>类型</dt>
          <dd>{hookKindLabel(definition)}</dd>
          <dt>运行</dt>
          <dd>{hookCommandLabel(definition)}</dd>
          <dt>事件</dt>
          <dd>{definition.events.map(hookEventLabel).join('、')}</dd>
          {tools ? (
            <>
              <dt>工具</dt>
              <dd>{tools}</dd>
            </>
          ) : null}
          <dt>超时</dt>
          <dd>{definition.timeout_ms / 1000} 秒</dd>
          <dt>指纹</dt>
          <dd title={preview.fingerprint ?? undefined}>
            <code>{shortFingerprint(preview.fingerprint)}</code>
          </dd>
        </dl>
      ) : null}
      {preview.files.length ? (
        <details className="hook-files">
          <summary>包内文件（{preview.files.length}）</summary>
          <ul>
            {preview.files.map((file) => (
              <li key={file.path}>
                <code>{file.path}</code> <span>{size(file.bytes)}</span>
              </li>
            ))}
          </ul>
        </details>
      ) : null}
      <p className="hook-warning" role="note">
        Hook 以你的身份运行，能读写你能读写的文件；只信任你了解的 Hook。
      </p>
      {trustable || onCancel ? (
        <div className="settings-card-actions">
          {trustable ? (
            <button
              type="button"
              className="button is-primary is-small"
              disabled={busy}
              onClick={onTrust}
            >
              {preview.registered ? '重新信任' : '信任并启用'}
            </button>
          ) : null}
          {onCancel ? (
            <button type="button" className="button is-small" disabled={busy} onClick={onCancel}>
              {trustable ? '取消' : '关闭'}
            </button>
          ) : null}
        </div>
      ) : null}
    </section>
  );
}
