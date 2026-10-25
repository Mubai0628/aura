import type { SkillView, WorkspaceId } from '@aura/contracts/generated';
import { useState } from 'react';

export type ChosenSkill = { id: string; name: string };

/** The most Skills one turn may have loaded (Core refuses more). */
export const MAX_CHOSEN_SKILLS = 20;

type Picker = { loading: boolean; skills: SkillView[]; error?: string };

/**
 * The Skills the user chose for the next turn, and the Skills that can be chosen now
 * (switched on, trusted and unchanged). Core checks the choice again when the message is sent.
 */
export function useSkillChoice(workspaceId: string | undefined) {
  const [chosen, setChosen] = useState<ChosenSkill[]>([]);
  const [picker, setPicker] = useState<Picker>();
  const open = async () => {
    setPicker({ loading: true, skills: [] });
    try {
      const result = await window.aura.v1.listSkills({
        workspace_id: (workspaceId ?? null) as WorkspaceId | null,
      });
      setPicker(
        result.ok
          ? { loading: false, skills: result.value.skills.filter((s) => s.state === 'ready') }
          : { loading: false, skills: [], error: 'Skill 列表读取失败，请重试。' },
      );
    } catch {
      setPicker({ loading: false, skills: [], error: 'Skill 列表读取失败，请重试。' });
    }
  };
  const add = (skill: SkillView) =>
    setChosen((current) =>
      current.some((entry) => entry.id === skill.id) || current.length >= MAX_CHOSEN_SKILLS
        ? current
        : [...current, { id: skill.id, name: skill.name ?? skill.relative }],
    );
  return {
    chosen,
    picker,
    open,
    close: () => setPicker(undefined),
    add,
    remove: (id: string) => setChosen((current) => current.filter((entry) => entry.id !== id)),
    clear: () => setChosen([]),
  };
}
