import type { AgentTeamPermissions } from '@aura/contracts/generated';
import { useId, useState } from 'react';
import { ConfirmationDialog } from '../ConfirmationDialog';
import { FULL_ACCESS_DELETES } from '../security/full-access-notice';
import { ToggleSwitch } from '../ui/ToggleSwitch';

type Mode = AgentTeamPermissions['worktree_mode'];
type ModeField = 'worktree_mode' | 'shared_mode';
type Switch =
  'allow_deletions' | 'allow_outside_writes' | 'allow_downloads' | 'main_agent_approves';

const MODES: readonly { mode: Mode; label: string }[] = [
  { mode: 'read_only', label: '只读' },
  { mode: 'workspace_read_write', label: '工作区读写' },
  { mode: 'full_access', label: '完全访问' },
];

const MODE_FIELDS: readonly { field: ModeField; label: string }[] = [
  { field: 'worktree_mode', label: '有工作副本的子 Agent' },
  { field: 'shared_mode', label: '共用工作区的子 Agent' },
];

const PRESETS: readonly { field: Switch; label: string }[] = [
  { field: 'allow_deletions', label: '命令删除文件' },
  { field: 'allow_outside_writes', label: '命令写工作区以外' },
  { field: 'allow_downloads', label: '下载' },
];

const FIELDS = [
  'worktree_mode',
  'shared_mode',
  'allow_deletions',
  'allow_outside_writes',
  'allow_downloads',
  'main_agent_approves',
] as const satisfies readonly (keyof AgentTeamPermissions)[];

const samePermissions = (one: AgentTeamPermissions, other: AgentTeamPermissions) =>
  FIELDS.every((field) => one[field] === other[field]);

/**
 * 设置 → 多 Agent: the mode a new sub-agent starts in, with a worktree of its own or in the
 * conversation's workspace; what its commands may do without asking; and whether the main agent
 * answers its questions first. Each change is saved at once; 完全访问 is confirmed first.
 */
export function AgentTeamPermissionsForm({
  permissions,
  defaults,
  disabled,
  onSave,
}: {
  permissions: AgentTeamPermissions | undefined;
  defaults: AgentTeamPermissions | undefined;
  disabled: boolean;
  onSave: (permissions: AgentTeamPermissions) => void;
}) {
  const baseId = useId();
  const [confirm, setConfirm] = useState<ModeField>();
  const change = (next: Partial<AgentTeamPermissions>) => {
    if (permissions !== undefined) onSave({ ...permissions, ...next });
  };
  const pick = (field: ModeField, mode: Mode) => {
    if (mode === 'full_access') setConfirm(field);
    else change({ [field]: mode });
  };
  const toggle = (field: Switch, label: string) => (
    <label className="agent-team-settings-toggle" key={field}>
      <ToggleSwitch
        checked={permissions?.[field]}
        disabled={disabled}
        label={label}
        onCheckedChange={(checked) => change({ [field]: checked })}
      />
      <span>{label}</span>
    </label>
  );
  return (
    <>
      <section
        aria-label="子 Agent 的默认档位"
        className="settings-surface agent-team-settings-group"
      >
        <h4>默认档位</h4>
        {MODE_FIELDS.map(({ field, label }) => {
          const id = `${baseId}-${field}`;
          return (
            <div className="agent-team-settings-field" key={field}>
              <label htmlFor={id}>{label}</label>
              <select
                className="agent-team-settings-select"
                disabled={disabled || permissions === undefined}
                id={id}
                onChange={(event) => pick(field, event.target.value as Mode)}
                value={permissions?.[field] ?? ''}
              >
                {permissions === undefined ? <option value="" /> : null}
                {MODES.map(({ mode, label: name }) => (
                  <option key={mode} value={mode}>
                    {name}
                  </option>
                ))}
              </select>
            </div>
          );
        })}
      </section>
      <section
        aria-label="子 Agent 不用问的事"
        className="settings-surface agent-team-settings-group"
      >
        <h4>不用问</h4>
        {PRESETS.map(({ field, label }) => toggle(field, label))}
      </section>
      <section aria-label="主 Agent 代批" className="settings-surface agent-team-settings-group">
        {toggle('main_agent_approves', '主 Agent 代批')}
        <p>
          主 Agent 可在当前任务内委派已授权路径的只读操作，也可代批子 Agent
          工作副本内符合条件的操作。涉及敏感路径的审批仍由你决定。
        </p>
      </section>
      {permissions !== undefined &&
      defaults !== undefined &&
      !samePermissions(permissions, defaults) ? (
        <div className="settings-card-actions">
          <button
            className="button is-small"
            disabled={disabled}
            onClick={() => onSave(defaults)}
            type="button"
          >
            恢复默认权限
          </button>
        </div>
      ) : null}
      {confirm !== undefined ? (
        <ConfirmationDialog
          busy={false}
          title="子 Agent 默认完全访问"
          titleId={`${baseId}-full-access`}
          confirmLabel="启用完全访问"
          description={`新开的子 Agent 可联网，并在已授权范围内修改或删除文件。${FULL_ACCESS_DELETES}`}
          onCancel={() => setConfirm(undefined)}
          onConfirm={() => {
            change({ [confirm]: 'full_access' });
            setConfirm(undefined);
          }}
        />
      ) : null}
    </>
  );
}
