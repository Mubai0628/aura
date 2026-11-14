import type { SkillView } from '@aura/contracts/generated';
import { useEffect, useRef } from 'react';
import { AppIcon } from '../app-icons';
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

/**
 * The usable Skills to choose from, by name; only ones the user trusted and left switched on.
 * Choosing one keeps it open; a press anywhere outside it, or Esc, closes it.
 */
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
  const root = useRef<HTMLElement>(null);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) onClose();
    };
    const escape = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose();
    };
    document.addEventListener('pointerdown', outside, true);
    document.addEventListener('keydown', escape);
    return () => {
      document.removeEventListener('pointerdown', outside, true);
      document.removeEventListener('keydown', escape);
    };
  }, [onClose]);
  const full = chosen.length >= MAX_CHOSEN_SKILLS;
  return (
    <section ref={root} className="composer-skill-picker composer-popover" aria-label="选择 Skill">
      <p>Skill</p>
      {picker.loading ? <p role="status">正在读取…</p> : null}
      {picker.error ? <p role="alert">{picker.error}</p> : null}
      {!picker.loading && !picker.error && !picker.skills.length ? <p>没有可用的 Skill</p> : null}
      <ul aria-label="可用的 Skill">
        {picker.skills.map((skill) => {
          const taken = chosen.some((entry) => entry.id === skill.id);
          // A pinned Skill sent at the start of every request is already loaded.
          const loaded = Boolean(skill.pinned && !skill.pinned_left_out);
          return (
            <li key={skill.id}>
              <button
                type="button"
                disabled={taken || full || loaded}
                onClick={() => onChoose(skill)}
              >
                <span>{skill.name ?? skill.relative}</span>
                {taken || loaded ? (
                  <>
                    <AppIcon name="check" size={13} aria-hidden="true" />
                    <span className="sr-only">{loaded ? '已加载' : '已选'}</span>
                  </>
                ) : null}
              </button>
            </li>
          );
        })}
      </ul>
      {full ? <p role="status">最多 {MAX_CHOSEN_SKILLS} 个</p> : null}
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
