import type { SkillView } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { skillSourceLabel } from '../skill-settings/skill-labels';
import { MAX_CHOSEN_SKILLS, type ChosenSkill } from './useSkillChoice';
import './composer-skills.css';

/** The Skills chosen for the next turn, each removable. */
export function SkillChips({
  chosen,
  onRemove,
}: {
  chosen: readonly ChosenSkill[];
  onRemove: (id: string) => void;
}) {
  if (!chosen.length) return null;
  return (
    <div className="composer-skill-chips" role="group" aria-label="本轮使用的 Skill">
      {chosen.map((skill) => (
        <span className="composer-skill-chip" key={skill.id}>
          Skill：{skill.name}
          <button
            type="button"
            aria-label={`移除 Skill ${skill.name}`}
            onClick={() => onRemove(skill.id)}
          >
            <AppIcon name="close" size={13} />
          </button>
        </span>
      ))}
    </div>
  );
}

/** The usable Skills to choose from; only ones the user trusted and left switched on. */
export function SkillPicker({
  picker,
  chosen,
  onChoose,
  onClose,
}: {
  picker: { loading: boolean; skills: readonly SkillView[]; error?: string };
  chosen: readonly ChosenSkill[];
  onChoose: (skill: SkillView) => void;
  onClose: () => void;
}) {
  const full = chosen.length >= MAX_CHOSEN_SKILLS;
  return (
    <section className="composer-skill-picker composer-popover" aria-label="选择 Skill">
      <header>
        <strong>选择本轮使用的 Skill</strong>
        <button type="button" onClick={onClose}>
          关闭
        </button>
      </header>
      <p className="composer-skill-picker-note">
        模型在这一轮会按它的说明工作；Skill 不给任何权限，工具照常按会话模式和审批执行。
      </p>
      {picker.loading ? <p role="status">正在读取 Skill…</p> : null}
      {picker.error ? <p role="alert">{picker.error}</p> : null}
      {!picker.loading && !picker.error && !picker.skills.length ? (
        <p>没有可用的 Skill：在 设置 → Skill 里查看并信任后才能选择。</p>
      ) : null}
      <ul aria-label="可用的 Skill">
        {picker.skills.map((skill) => {
          const taken = chosen.some((entry) => entry.id === skill.id);
          return (
            <li key={skill.id}>
              <button type="button" disabled={taken || full} onClick={() => onChoose(skill)}>
                <span>{skill.name ?? skill.relative}</span>
                <small>
                  {skillSourceLabel(skill.source)}
                  {skill.description ? ` · ${skill.description}` : ''}
                </small>
              </button>
            </li>
          );
        })}
      </ul>
      {full ? <p role="status">一次最多选择 {MAX_CHOSEN_SKILLS} 个 Skill。</p> : null}
    </section>
  );
}

/** The Skills chosen with a message, shown with it in the conversation and the queue. */
export function ChosenSkillsLine({
  skills,
}: {
  skills: ReadonlyArray<{ name: string }> | undefined;
}) {
  const names = [...new Set((skills ?? []).map((skill) => skill.name))];
  return names.length ? <p className="chosen-skills-line">已选 Skill：{names.join('、')}</p> : null;
}
