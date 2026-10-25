import type { SkillPreview } from '@aura/contracts/generated';
import { bytesLabel } from './skill-labels';

/** What trusting a Skill covers: exactly this SKILL.md and these files. The button that opened
 * it closes it, so the preview has no close button of its own. */
export function SkillPreviewPanel({
  preview,
  busy,
  onTrust,
}: {
  preview: SkillPreview;
  busy: boolean;
  onTrust?: (() => void) | undefined;
}) {
  const { skill } = preview;
  return (
    <section
      className="skill-preview settings-panel"
      aria-label={`预览 Skill ${skill.name ?? skill.relative}`}
    >
      <p>
        内容指纹 <code>{skill.fingerprint?.slice(0, 16) ?? '—'}</code> · {skill.file_count} 个文件 ·{' '}
        {bytesLabel(skill.bytes)}
      </p>
      {skill.missing_references.length ? (
        <p className="skill-row-warning">
          SKILL.md 提到但文件夹里没有：{skill.missing_references.join('、')}
        </p>
      ) : null}
      <details open>
        <summary>SKILL.md</summary>
        <pre>{preview.skill_md}</pre>
      </details>
      <details>
        <summary>文件（{preview.files.length}）</summary>
        <ul>
          {preview.files.map((file) => (
            <li key={file.path}>
              <code>{file.path}</code> {bytesLabel(file.bytes)}
            </li>
          ))}
        </ul>
      </details>
      {onTrust ? (
        <p>信任的是上面这份内容；以后任何文件改动都需要你重新信任，模型才能再用它。</p>
      ) : null}
      {onTrust ? (
        <div className="settings-card-actions">
          <button
            type="button"
            className="button is-primary is-small"
            disabled={busy}
            onClick={onTrust}
          >
            信任并启用
          </button>
        </div>
      ) : null}
    </section>
  );
}
