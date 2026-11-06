import type { SkillView, WorkspaceId } from '@aura/contracts/generated';
import { useCallback, useState } from 'react';
import { useKeptDraft } from '../agent-session/kept-draft';

export type ChosenSkill = { id: string; name: string };

const NONE: ChosenSkill[] = [];

/** The most Skills one turn may have loaded (Core refuses more). */
export const MAX_CHOSEN_SKILLS = 20;

type Picker = { loading: boolean; skills: SkillView[]; error?: string };

/**
 * The Skills the user chose for the next turn, and the Skills that can be chosen now
 * (switched on, trusted and unchanged). Core checks the choice again when the message is sent.
 * The choice is kept for the conversation `draftKey` names while Aura runs.
 */
export function useSkillChoice(workspaceId: string | undefined, draftKey?: string) {
  const [chosen, setChosen] = useKeptDraft('skills', draftKey, NONE);
  const [picker, setPicker] = useState<Picker>();
  // The Skills usable now, for the picker and for the "@" list.
  const usable = useCallback(async (): Promise<SkillView[]> => {
    const result = await window.aura.v1.listSkills({
      workspace_id: (workspaceId ?? null) as WorkspaceId | null,
    });
    if (!result.ok) throw new Error('skills');
    return result.value.skills.filter((s) => s.state === 'ready');
  }, [workspaceId]);
  const open = async () => {
    setPicker({ loading: true, skills: [] });
    try {
      setPicker({ loading: false, skills: await usable() });
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
    usable,
    open,
    close: () => setPicker(undefined),
    add,
    remove: (id: string) => setChosen((current) => current.filter((entry) => entry.id !== id)),
    clear: () => setChosen([]),
  };
}
