import { useEffect, useState } from 'react';
import type {
  SkillPreview,
  SkillRegistryView,
  SkillView,
  WorkspaceId,
} from '@aura/contracts/generated';
import type { ResultEnvelope } from '../../../shared/aura-api';
import { AppIcon } from '../app-icons';
import { SkillDetails, SkillHeading, type SkillChange } from './SkillDetails';
import {
  skillError,
  skillName,
  skillSourceLabel,
  skillStateTone,
  skillUsable,
} from './skill-labels';
import { SettingsItemDialog, SettingsRow, SettingsRowOpen } from '../ui/SettingsItem';
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
  const [opened, setOpened] = useState<string>();
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
  // A Skill that leaves the list, removed here or no longer found, takes its dialog with it.
  const shown = opened === undefined ? undefined : skills.find((skill) => skill.id === opened);
  const close = () => {
    setOpened(undefined);
    setRemoving(undefined);
    setPreview(undefined);
  };
  // A dialog shows what is done in it, never the outcome of an earlier action on another item.
  const openItem = (id: string) => {
    setNotice(undefined);
    setOpened(id);
  };
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
  // The listed workspace's two Skill folders are trusted, or no longer trusted, as a whole.
  const trustWorkspace = (trusted: boolean) =>
    run('workspace', async () => {
      if (!workspace) return undefined;
      const scope = { workspace_id: workspace, expected_revision: revision };
      return trusted
        ? applied(await window.aura.v1.trustWorkspaceSkills(scope), '已信任当前工作区的 Skill')
        : applied(await window.aura.v1.untrustWorkspaceSkills(scope), '已撤销对当前工作区的信任');
    });
  const pin = (skill: SkillView, pinned: boolean) =>
    run(`pin:${skill.id}`, async () =>
      applied(
        await window.aura.v1.setSkillPinned({ ...request(skill), pinned }),
        pinned ? `已固定加载 ${skillName(skill)}` : `已取消固定加载 ${skillName(skill)}`,
      ),
    );
  const change = (skill: SkillView, action: SkillChange) =>
    run(`${action}:${skill.id}`, async () => {
      const api = window.aura.v1;
      switch (action) {
        case 'enable':
        case 'disable':
          return applied(
            await api.setSkillEnabled({ ...request(skill), enabled: action === 'enable' }),
            action === 'enable' ? `已启用 ${skillName(skill)}` : `已停用 ${skillName(skill)}`,
          );
        case 'untrust':
          return applied(
            await api.untrustSkill(request(skill)),
            `已撤销对 ${skillName(skill)} 的信任`,
          );
        case 'remove': {
          setRemoving(undefined);
          if (preview?.skill.id === skill.id) setPreview(undefined);
          const done = applied(
            await api.removeSkill(request(skill)),
            skill.source === 'aura_store'
              ? `已移除 ${skillName(skill)}`
              : `已删除 ${skillName(skill)}`,
          );
          // A Skill imported again later under the same id does not reopen this dialog.
          if (!done.alert) close();
          return done;
        }
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
        '已导入，可以使用',
      );
    });

  const row = (skill: SkillView, index: number) => {
    const tone = skillStateTone(skill);
    return (
      <SettingsRow
        key={skill.id}
        state={skill.state}
        tone={tone === 'success' ? undefined : tone}
        index={index}
        headingClassName="skill-row-heading"
        heading={<SkillHeading skill={skill} />}
        summary={skill.description || undefined}
        alerts={
          skill.problems.length ? (
            <p className="settings-inline-alert" role="alert">
              {skill.problems.join('；')}
            </p>
          ) : null
        }
        actions={
          <>
            {skillUsable(skill) ? (
              <button
                type="button"
                className="button is-small"
                disabled={busy !== undefined}
                onClick={() => void change(skill, skill.enabled ? 'disable' : 'enable')}
              >
                {skill.enabled ? '停用' : '启用'}
              </button>
            ) : null}
            <SettingsRowOpen name={skillName(skill)} onOpen={() => openItem(skill.id)} />
          </>
        }
      />
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
          {workspace && registry ? (
            <button
              type="button"
              className="button"
              disabled={!coreReady || busy !== undefined}
              onClick={() => void trustWorkspace(!registry.workspace_trusted)}
            >
              {registry.workspace_trusted ? '撤销对当前工作区的信任' : '信任当前工作区的 Skill'}
            </button>
          ) : null}
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
          {skills.some((skill) => skill.pinned) ? (
            <p className="skill-pin-total">
              {`固定加载 约 ${registry.pinned_tokens ?? 0} / ${registry.pinned_limit ?? 0} token`}
            </p>
          ) : null}
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
      {shown ? (
        <SettingsItemDialog
          title={skillName(shown)}
          heading={<SkillHeading skill={shown} named={false} />}
          headingClassName="skill-row-heading"
          notice={notice}
          onClose={close}
        >
          <SkillDetails
            skill={shown}
            locked={busy !== undefined}
            workspaceTrusted={Boolean(registry?.workspace_trusted)}
            preview={preview?.skill.id === shown.id ? preview : undefined}
            removing={removing === shown.id}
            onPreview={() => void open(shown)}
            onClosePreview={() => setPreview(undefined)}
            onChange={(action) => void change(shown, action)}
            onPin={(pinned) => void pin(shown, pinned)}
            onReveal={() => void reveal(shown)}
            onAskRemove={() => setRemoving(shown.id)}
            onCancelRemove={() => setRemoving(undefined)}
          />
        </SettingsItemDialog>
      ) : null}
    </section>
  );
}
