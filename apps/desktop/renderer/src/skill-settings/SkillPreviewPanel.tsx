import type { SkillPreview } from '@aura/contracts/generated';
import { bytesLabel } from './skill-labels';

/** A Skill as it is now: its SKILL.md and its files. The button that opened it closes it, so the
 * preview has no close button of its own. */
export function SkillPreviewPanel({ preview }: { preview: SkillPreview }) {
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
    </section>
  );
}
