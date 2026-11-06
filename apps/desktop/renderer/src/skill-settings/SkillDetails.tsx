import type { SkillPreview, SkillView } from '@aura/contracts/generated';
import { AppIcon } from '../app-icons';
import { SkillPreviewPanel } from './SkillPreviewPanel';
import {
  skillName,
  skillPlugin,
  skillSourceLabel,
  skillStateLabel,
  skillStateTone,
  skillUsable,
} from './skill-labels';

export type SkillChange = 'enable' | 'disable' | 'untrust' | 'remove';

/** Where a Skill comes from, its state and whether it is loaded at the start of every request. */
export function SkillHeading({ skill, named = true }: { skill: SkillView; named?: boolean }) {
  const plugin = skillPlugin(skill);
  return (
    <>
      {named ? <strong>{skillName(skill)}</strong> : null}
      <span className="settings-tag skill-row-source">
        {plugin === undefined ? skillSourceLabel(skill.source) : `插件 ${plugin}`}
      </span>
      <span className="status-pill skill-row-state" data-tone={skillStateTone(skill)}>
        {skillStateLabel(skill)}
      </span>
      {skill.pinned && skill.state === 'ready' ? (
        <span
          className="status-pill skill-row-pin"
          data-tone={skill.pinned_left_out ? 'warning' : undefined}
        >
          {skill.pinned_left_out
            ? '超出上限，这次没有固定加载'
            : `固定加载 约 ${skill.pin_tokens ?? 0} token`}
        </span>
      ) : null}
    </>
  );
}

/** One Skill in its dialog: where it is, what it says it needs, and the user's choices. */
export function SkillDetails({
  skill,
  locked,
  workspaceTrusted,
  preview,
  removing,
  onPreview,
  onClosePreview,
  onChange,
  onPin,
  onReveal,
  onAskRemove,
  onCancelRemove,
}: {
  skill: SkillView;
  locked: boolean;
  workspaceTrusted: boolean;
  preview: SkillPreview | undefined;
  removing: boolean;
  onPreview: () => void;
  onClosePreview: () => void;
  onChange: (action: SkillChange) => void;
  onPin: (pinned: boolean) => void;
  onReveal: () => void;
  onAskRemove: () => void;
  onCancelRemove: () => void;
}) {
  const usable = skillUsable(skill);
  const plugin = skillPlugin(skill);
  // A workspace Skill trusted alone before the workspace was trusted as a whole.
  const trustedAlone =
    Boolean(skill.trusted_fingerprint) &&
    (skill.source === 'workspace' || skill.source === 'claude_workspace') &&
    !workspaceTrusted;
  const pinnable = skill.state === 'ready' || Boolean(skill.pinned);
  const removal = skill.source === 'aura_store' ? '移除' : '删除';
  return (
    <div className="skill-details">
      {skill.description ? <p>{skill.description}</p> : null}
      <p className="skill-row-path">
        <code>{skill.path}</code>
      </p>
      {skill.allowed_tools.length ? (
        <p className="skill-row-tools">
          它说需要的工具：{skill.allowed_tools.join('、')}（只是说明，不会因此允许任何工具）
        </p>
      ) : null}
      {skill.problems.length ? (
        <p className="settings-inline-alert" role="alert">
          {skill.problems.join('；')}
        </p>
      ) : null}
      {skill.warnings.length ? (
        <p className="skill-row-warning">{skill.warnings.join('；')}</p>
      ) : null}
      <div className="settings-card-actions">
        {usable ? (
          // The button that opens the preview also closes it.
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            aria-expanded={preview !== undefined}
            onClick={preview ? onClosePreview : onPreview}
          >
            {preview ? '收起' : '预览'}
          </button>
        ) : null}
        {usable ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange(skill.enabled ? 'disable' : 'enable')}
          >
            {skill.enabled ? '停用' : '启用'}
          </button>
        ) : null}
        {pinnable ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            aria-pressed={Boolean(skill.pinned)}
            onClick={() => onPin(!skill.pinned)}
          >
            {skill.pinned ? '取消固定加载' : '固定加载'}
          </button>
        ) : null}
        {trustedAlone && plugin === undefined ? (
          <button
            type="button"
            className="button is-small"
            disabled={locked}
            onClick={() => onChange('untrust')}
          >
            撤销信任
          </button>
        ) : null}
        <button
          type="button"
          className="button is-ghost is-small"
          disabled={locked}
          onClick={onReveal}
        >
          <AppIcon name="folder" size={14} />
          打开文件夹
        </button>
        {skill.source === 'plugin' || skill.source === 'aura_builtin' ? null : (
          <button
            type="button"
            className="button is-danger is-small skill-remove"
            disabled={locked}
            onClick={onAskRemove}
          >
            {removal}
          </button>
        )}
      </div>
      {removing ? (
        <div
          role="alertdialog"
          aria-label={`${removal} ${skillName(skill)}`}
          className="settings-panel is-danger"
        >
          {skill.source === 'aura_store' ? (
            <p>移除后 Aura 里的这份副本会被删除；你当初导入的原文件夹不受影响。</p>
          ) : (
            <p>
              删除后，这个 Skill 的文件夹会移到回收站：{skill.path}
              {skill.source === 'agent_global' ? ' 其他使用 .agents 的工具也会看不到它。' : null}
            </p>
          )}
          <div className="settings-card-actions">
            <button
              type="button"
              className="button is-danger is-solid is-small"
              onClick={() => onChange('remove')}
            >
              {removal}
            </button>
            <button type="button" className="button is-small" onClick={onCancelRemove}>
              取消
            </button>
          </div>
        </div>
      ) : null}
      {preview ? <SkillPreviewPanel preview={preview} /> : null}
    </div>
  );
}
