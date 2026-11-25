import type { AgentTeamMemberView } from '@aura/contracts/generated';
import { useState } from 'react';
import { memberPhase, PHASE_WORDS } from '../agent-team-words';

type Preference = { collapsed: boolean; stopped: boolean; lastUsed: number };
type Preferences = Record<string, Preference>;

function read(key: string): Preferences {
  try {
    const saved: unknown = JSON.parse(localStorage.getItem(key) ?? '{}');
    if (!saved || typeof saved !== 'object' || Array.isArray(saved)) return {};
    return Object.fromEntries(
      Object.entries(saved).filter(
        ([id, entry]) =>
          id.startsWith('ses_') &&
          typeof entry?.collapsed === 'boolean' &&
          typeof entry.stopped === 'boolean' &&
          Number.isFinite(entry.lastUsed),
      ),
    );
  } catch {
    return {};
  }
}

export function useMemberLayout(parent: string, members: readonly AgentTeamMemberView[]) {
  const key = `aura.agent-canvas.members.v1:${parent}`;
  const [preferences, setPreferences] = useState(() => read(key));
  const hidden = (member: AgentTeamMemberView) => {
    const stopped = member.state === 'stopped';
    const saved = preferences[member.session_id];
    return saved?.stopped === stopped ? saved.collapsed : stopped;
  };
  const set = (items: readonly AgentTeamMemberView[], collapsed: boolean) => {
    const at = Date.now();
    const next = { ...preferences, ...read(key) };
    for (const member of items) {
      next[member.session_id] = {
        stopped: member.state === 'stopped',
        collapsed,
        lastUsed: collapsed ? (next[member.session_id]?.lastUsed ?? 0) : at,
      };
    }
    try {
      localStorage.setItem(key, JSON.stringify(next));
    } catch {
      // Session interaction can still continue when local preferences cannot be written.
    }
    setPreferences(next);
  };
  const lastUsed = (member: AgentTeamMemberView) =>
    Math.max(
      preferences[member.session_id]?.lastUsed ?? 0,
      Date.parse(member.activity_at ?? '') || 0,
    );
  const folded = members
    .filter(hidden)
    .sort(
      (first, second) =>
        lastUsed(second) - lastUsed(first) || first.session_id.localeCompare(second.session_id),
    );
  const byPhase = new Map<string, AgentTeamMemberView[]>();
  for (const member of folded) {
    const name = PHASE_WORDS[memberPhase(member)];
    const group = byPhase.get(name) ?? [];
    group.push(member);
    byPhase.set(name, group);
  }
  return { folded, byPhase, visible: members.filter((member) => !hidden(member)), set };
}
