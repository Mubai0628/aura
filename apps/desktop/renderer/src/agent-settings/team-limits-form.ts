import type { AgentTeamLimits, AgentTeamSettingsView } from '@aura/contracts/generated';

export type Field = keyof AgentTeamLimits;
export type Draft = Record<Field, string> & { unlimited: boolean };
export const FIELDS: readonly { field: Field; label: string; unit: string }[] = [
  { field: 'max_running', label: '同时运行的子 Agent', unit: '个' },
  { field: 'token_budget', label: '每个子 Agent 的 token 上限', unit: 'token' },
  { field: 'minutes_per_task', label: '每个任务的用时上限', unit: '分钟' },
  { field: 'cpu_percent', label: '子 Agent 命令合计的 CPU 占比', unit: '%' },
];
export const limitOf = (limits: AgentTeamLimits, field: Field): number | null =>
  limits[field] ?? null;
export const toDraft = (limits: AgentTeamLimits): Draft => ({
  max_running: String(limits.max_running),
  token_budget: limits.token_budget == null ? '' : String(limits.token_budget),
  minutes_per_task: String(limits.minutes_per_task),
  cpu_percent: String(limits.cpu_percent),
  unlimited: limits.token_budget == null,
});
const valueOf = (draft: Draft, view: AgentTeamSettingsView, field: Field): number | undefined => {
  const text = draft[field].trim();
  const number = text === '' ? Number.NaN : Number(text);
  const [least, most] = [limitOf(view.minimum, field) ?? 0, limitOf(view.maximum, field) ?? 0];
  return Number.isInteger(number) && number >= least && number <= most ? number : undefined;
};
export const takes = (draft: Draft, view: AgentTeamSettingsView, field: Field) =>
  (field === 'token_budget' && draft.unlimited) || valueOf(draft, view, field) !== undefined;
export const parse = (draft: Draft, view: AgentTeamSettingsView): AgentTeamLimits | undefined => {
  if (!FIELDS.every(({ field }) => takes(draft, view, field))) return undefined;
  const value = (field: Field) => valueOf(draft, view, field) ?? 0;
  return {
    max_running: value('max_running'),
    token_budget: draft.unlimited ? null : value('token_budget'),
    minutes_per_task: value('minutes_per_task'),
    cpu_percent: value('cpu_percent'),
  };
};
export const same = (one: AgentTeamLimits, other: AgentTeamLimits) =>
  FIELDS.every(({ field }) => limitOf(one, field) === limitOf(other, field));
