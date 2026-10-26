import { useEffect, useState } from 'react';
import type {
  SkillPreview,
  SkillRegistryView,
  SkillView,
  WorkspaceId,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { SkillPreviewPanel } from './SkillPreviewPanel';
import type { CSSProperties } from 'react';
import { AppIcon } from '../app-icons';
import { skillError, skillSourceLabel, skillStateLabel, skillStateTone } from './skill-labels';
import { ListSentinel, SkeletonRows } from '../ui/SkeletonRows';
import { useIncrementalList } from '../ui/useIncrementalList';
import './skill-settings.css';

const NO_SKILLS: readonly SkillView[] = [];

type Notice = { alert: boolean; text: string };

/** Settings → Skill: what Skills Aura found, what each one is, and the user's choices. */
export function SkillSettings({
  coreReady,
  workspaceId,
}: {
  coreReady: boolean;
  workspaceId?: WorkspaceId | undefined;
}) {
  const [registry, setRegistry] = useState<SkillRegistryView>();
  const [loadError, setLoadError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [busy, setBusy] = useState<string>();
  const [notice, setNotice] = useState<Notice>();
  const [preview, setPreview] = useState<SkillPreview>();
  const [removing, setRemoving] = useState<string>();
  const workspace = workspaceId ?? null;
  useEffect(() => {
    if (!coreReady) return;
    let live = true;
    void window.aura.v1.listSkills({ workspace_id: workspace }).then(
      (result) => {
        if (!live) return;
        if (result.ok) {
          setRegistry(result.value);
          setLoadError(undefined);
        } else setLoadError(skillError(result.error.message, 'Skill 列表读取失败，请重试。'));
      },
      (reason: unknown) => {
        if (live) setLoadError(skillError(reason, 'Skill 列表读取失败，请重试。'));
      },
    );
    return () => {
      live = false;
    };
  }, [coreReady, reload, workspace]);

  async function run(label: string, work: () => Promise<Notice | undefined>) {
    if (busy) return;
    setBusy(label);
    setNotice(undefined);
    try {
      const outcome = await work();
      if (outcome) setNotice(outcome);
      if (outcome?.alert) setReload((value) => value + 1);
    } catch (reason) {
      setNotice({ alert: true, text: skillError(reason, '操作结果未确认，请刷新后核对。') });
      setReload((value) => value + 1);
    } finally {
      setBusy(undefined);
    }
  }
  const applied = (result: ResultEnvelope<SkillRegistryView>, done: string): Notice => {
    if (!result.ok) return { alert: true, text: skillError(result.error.message) };
    setRegistry(result.value);
    return { alert: false, text: done };
  };
  const revision = registry?.revision ?? 0;
  const skills = registry?.skills ?? NO_SKILLS;
  const pages = useIncrementalList(skills);
  const name = (skill: SkillView) => skill.name ?? skill.relative;
  const request = (skill: SkillView) => ({
    id: skill.id,
    workspace_id: skill.workspace_id ?? null,
    expected_revision: revision,
  });

  const open = (skill: SkillView) =>
    run(`preview:${skill.id}`, async () => {
      const result = await window.aura.v1.previewSkill({
        id: skill.id,
        workspace_id: skill.workspace_id ?? null,
      });
      if (!result.ok) return { alert: true, text: skillError(result.error.message) };
      setPreview(result.value);
      return undefined;
    });
  const trust = (shown: SkillPreview) =>
    run(`trust:${shown.skill.id}`, async () => {
      const done = applied(
        await window.aura.v1.trustSkill({
          ...request(shown.skill),
          expected_fingerprint: shown.skill.fingerprint ?? '',
        }),
        `已信任并启用 ${name(shown.skill)}`,
      );
      if (!done.alert) setPreview(undefined);
      return done;
    });
  const change = (skill: SkillView, action: 'enable' | 'disable' | 'untrust' | 'remove') =>
    run(`${action}:${skill.id}`, async () => {
      const api = window.aura.v1;
      switch (action) {
        case 'enable':
        case 'disable':
          return applied(
            await api.setSkillEnabled({ ...request(skill), enabled: action === 'enable' }),
            action === 'enable' ? `已启用 ${name(skill)}` : `已停用 ${name(skill)}`,
          );
        case 'untrust':
          return applied(await api.untrustSkill(request(skill)), `已撤销对 ${name(skill)} 的信任`);
        case 'remove':
          setRemoving(undefined);
          return applied(
            await api.removeSkill(request(skill)),
            skill.source === 'aura_store' ? `已移除 ${name(skill)}` : `已删除 ${name(skill)}`,
          );
      }
    });
  // Main opens the folder Core lists for this Skill; the page never sends a path.
  const reveal = (skill: SkillView) =>
    run(`folder:${skill.id}`, async () => {
      const result = await window.aura.v1.openSkillDirectory({
        id: skill.id,
        workspace_id: skill.workspace_id ?? null,
      });
      if (!result.ok) return { alert: true, text: skillError(result.error.message) };
      return { alert: !result.value.completed, text: result.value.message };
    });
  // Main shows the dialog; Core copies a folder or unpacks a ZIP file and checks it.
  const importFrom = (kind: 'folder' | 'archive') =>
    run('import', async () => {
      const chosen =
        kind === 'folder'
          ? await window.aura.v1.selectSkillDirectory({})
          : await window.aura.v1.selectSkillArchive({});
      if (!chosen.ok) return { alert: true, text: skillError(chosen.error.message) };
      if (!chosen.value.path) return undefined;
      return applied(
        await window.aura.v1.importSkill({ path: chosen.value.path, expected_revision: revision }),
        '已导入；查看并信任后模型才能使用',
      );
    });

  const row = (skill: SkillView, index: number) => {
    const locked = busy !== undefined;
    const usable = skill.state !== 'invalid' && skill.state !== 'unavailable';
    // A plugin's Skill is trusted and removed with its plugin.
    const plugin = skill.source === 'plugin' ? (skill.plugin ?? '') : undefined;
    const needsTrust =
      plugin === undefined &&
      usable &&
      (skill.trusted_fingerprint == null || skill.state === 'content_changed');
    const tone = skillStateTone(skill);
    const shown = preview?.skill.id === skill.id;
    return (
      <li
        key={skill.id}
        className="settings-card"
        data-state={skill.state}
        data-tone={tone === 'success' ? undefined : tone}
        style={{ '--item': index } as CSSProperties}
      >
        <div className="settings-card-heading skill-row-heading">
          <strong>{name(skill)}</strong>
          <span className="settings-tag skill-row-source">
            {plugin === undefined ? skillSourceLabel(skill.source) : `插件 ${plugin}`}
          </span>
          <span className="status-pill skill-row-state" data-tone={tone}>
            {skillStateLabel(skill)}
          </span>
        </div>
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
              className={needsTrust && !shown ? 'button is-primary is-small' : 'button is-small'}
              disabled={locked}
              aria-expanded={shown}
              onClick={() => (shown ? setPreview(undefined) : void open(skill))}
            >
              {shown
                ? '收起'
                : needsTrust
                  ? skill.state === 'content_changed'
                    ? '查看变化并重新信任'
                    : '查看并信任'
                  : '预览'}
            </button>
          ) : null}
          {usable && !needsTrust ? (
            <button
              type="button"
              className="button is-small"
              disabled={locked}
              onClick={() => void change(skill, skill.enabled ? 'disable' : 'enable')}
            >
              {skill.enabled ? '停用' : '启用'}
            </button>
          ) : null}
          {skill.trusted_fingerprint && plugin === undefined ? (
            <button
              type="button"
              className="button is-small"
              disabled={locked}
              onClick={() => void change(skill, 'untrust')}
            >
              撤销信任
            </button>
          ) : null}
          <button
            type="button"
            className="button is-ghost is-small"
            disabled={locked}
            onClick={() => void reveal(skill)}
          >
            <AppIcon name="folder" size={14} />
            打开文件夹
          </button>
          {skill.source === 'plugin' ? null : (
            <button
              type="button"
              className="button is-danger is-small skill-remove"
              disabled={locked}
              onClick={() => setRemoving(skill.id)}
            >
              {skill.source === 'aura_store' ? '移除' : '删除'}
            </button>
          )}
        </div>
        {removing === skill.id ? (
          <div
            role="alertdialog"
            aria-label={`${skill.source === 'aura_store' ? '移除' : '删除'} ${name(skill)}`}
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
                onClick={() => void change(skill, 'remove')}
              >
                {skill.source === 'aura_store' ? '移除' : '删除'}
              </button>
              <button
                type="button"
                className="button is-small"
                onClick={() => setRemoving(undefined)}
              >
                取消
              </button>
            </div>
          </div>
        ) : null}
        {shown && preview ? (
          <SkillPreviewPanel
            preview={preview}
            busy={locked}
            onTrust={needsTrust ? () => void trust(preview) : undefined}
          />
        ) : null}
      </li>
    );
  };

  return (
    <section className="skill-settings settings-page" aria-label="Skill 设置">
      <header className="settings-content-heading">
        <div>
          <h2>Skill</h2>
        </div>
        <div className="settings-page-actions">
          <button
            type="button"
            className="button is-primary"
            disabled={!coreReady || busy !== undefined}
            onClick={() => void importFrom('folder')}
          >
            <AppIcon name="folder" size={15} />
            导入文件夹…
          </button>
          <button
            type="button"
            className="button"
            disabled={!coreReady || busy !== undefined}
            onClick={() => void importFrom('archive')}
          >
            导入 ZIP…
          </button>
          <button
            type="button"
            className="button is-ghost"
            aria-label="刷新"
            title="刷新"
            onClick={() => setReload((value) => value + 1)}
          >
            <AppIcon name="refresh" size={15} />
          </button>
        </div>
      </header>
      {notice ? (
        <p
          className={notice.alert ? 'settings-status-line' : 'settings-status-line is-done'}
          role={notice.alert ? 'alert' : 'status'}
        >
          {notice.text}
        </p>
      ) : null}
      {registry?.issue ? (
        <p className="settings-status-line" role="alert">
          {registry.issue}
        </p>
      ) : null}
      {!coreReady ? (
        <p className="settings-status-line" role="status">
          Core 尚未就绪，Skill 管理暂不可用。
        </p>
      ) : loadError ? (
        <p className="settings-status-line" role="alert">
          {loadError}{' '}
          <button
            type="button"
            className="button is-small"
            onClick={() => setReload((value) => value + 1)}
          >
            重试
          </button>
        </p>
      ) : !registry ? (
        // The list's shape while it is read.
        <SkeletonRows label="正在读取 Skill" />
      ) : (
        <>
          <ul className="skill-roots" aria-label="Skill 位置">
            {registry.roots.map((root) => (
              <li key={`${root.source}:${root.path}`}>
                {skillSourceLabel(root.source)}：<code>{root.path}</code>
                {root.exists ? null : <span>（不存在或是链接，未读取）</span>}
              </li>
            ))}
          </ul>
          {skills.length ? (
            <ul className="skill-list settings-card-list" aria-label="Skill">
              {pages.shown.map(row)}
              {pages.more ? <ListSentinel sentinel={pages.sentinel} /> : null}
            </ul>
          ) : (
            <div className="settings-empty">
              <span className="settings-empty-icon" aria-hidden="true">
                <AppIcon name="file" size={22} />
              </span>
              <strong>还没有 Skill</strong>
              <p>导入一个含 SKILL.md 的文件夹或 ZIP 文件，或放进上面的位置后刷新。</p>
            </div>
          )}
        </>
      )}
    </section>
  );
}
