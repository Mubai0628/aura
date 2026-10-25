import { useState } from 'react';
import type {
  MemoryRepairPreview,
  MemoryValidation,
  MemoryVaultStatus,
  WorkspaceId,
} from '@aura/contracts/generated';
import type { useMemory } from './useMemory';
/** History records keep stable codes; people read what happened and who did it. */
const ACTIONS: Record<string, string> = {
  create: '新建',
  update: '修改',
  delete: '删除',
  repair: '修复',
  external_create: '在外部新建',
  external_edit: '在外部修改',
  external_delete: '在外部删除',
  interrupted_undo: '撤回中断的写入',
  interrupted_kept: '中断后已被改动，保留现状',
};
const ACTORS: Record<string, string> = {
  model_tool: '模型',
  automatic_maintenance: '自动整理',
  external: '外部编辑',
  settings_repair: '设置页',
  recovery: 'Aura 恢复',
};
export function VaultPanel({
  vault,
  workspaceId,
  state,
}: {
  vault: MemoryVaultStatus;
  workspaceId?: WorkspaceId | undefined;
  state: ReturnType<typeof useMemory>;
}) {
  const [validation, setValidation] = useState<MemoryValidation>();
  const [preview, setPreview] = useState<MemoryRepairPreview>();
  const [notice, setNotice] = useState<string>();
  const request = { scope: vault.scope, workspace_id: workspaceId ?? null };
  const title = vault.scope === 'global' ? '全局记忆' : '项目记忆';
  const simple = async (command: 'memory.vault.open_directory' | 'memory.vault.export') => {
    const result = await state.act(() => state.call(command, request));
    if (result) setNotice(result.message);
  };
  return (
    <section className="memory-vault settings-card" aria-label={title}>
      <header>
        <h3>{title}</h3>
        <span>
          {vault.valid} 条 · {vault.invalid} 个无效文件
        </span>
      </header>
      <p className="memory-directory">
        <code>{vault.directory}</code>
      </p>
      {!vault.exists ? <p>尚未创建；第一次写入时创建。</p> : null}
      {vault.rebuilding ? <p role="status">索引正在重建，结果可能不全。</p> : null}
      {/* Each Core notice already says what happened and what to do next. */}
      {vault.watcher_error ? <p role="alert">{vault.watcher_error}。</p> : null}
      <div className="memory-actions">
        <button
          type="button"
          disabled={state.busy || !vault.exists}
          onClick={() => void simple('memory.vault.open_directory')}
        >
          打开目录
        </button>
        <button
          type="button"
          disabled={state.busy || !vault.exists}
          onClick={() => void simple('memory.vault.export')}
        >
          导出 ZIP
        </button>
        <button
          type="button"
          disabled={state.busy}
          onClick={() =>
            void state.act(async () => {
              setPreview(undefined);
              const result = await state.call('memory.vault.validate', request);
              setValidation(result);
            })
          }
        >
          校验
        </button>
        <button
          type="button"
          disabled={state.busy}
          onClick={() =>
            void state.act(async () => {
              setValidation(undefined);
              setPreview(undefined);
              const result = await state.call('memory.vault.repair.preview', request);
              setPreview(result);
            })
          }
        >
          预览修复
        </button>
        <button
          type="button"
          disabled={state.busy}
          onClick={() =>
            void (async () => {
              const result = await state.act(
                () => state.call('memory.vault.rebuild', request),
                '索引已从文件重建',
              );
              if (result) await state.reload();
            })()
          }
        >
          重建索引
        </button>
      </div>
      {notice ? <p role="status">{notice}</p> : null}
      {validation ? (
        <div aria-label={`${title}校验结果`}>
          <p>
            校验通过 {validation.valid} 条；无效 {validation.invalid.length} 个。
          </p>
          <ul>
            {validation.invalid.map((file) => (
              <li key={file.path}>
                <code>{file.path}</code>：{file.reason}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
      {preview ? (
        <div className="memory-repair" aria-label={`${title}修复预览`}>
          <h4>修复预览</h4>
          <p>只修结构、缺省字段和关系，不改正文语义。应用前会再次核对文件哈希。</p>
          <ul>
            {preview.changes.map((change) => (
              <li key={change.path}>
                <code>{change.path}</code>：{change.changes.join('；')}
              </li>
            ))}
          </ul>
          {preview.manual.length ? (
            <>
              <h4>需要手动处理</h4>
              <ul>
                {preview.manual.map((file) => (
                  <li key={file.path}>
                    <code>{file.path}</code>：{file.reason}
                  </li>
                ))}
              </ul>
            </>
          ) : null}
          <button
            type="button"
            disabled={state.busy}
            onClick={() =>
              void (async () => {
                const token = preview.preview_token;
                setPreview(undefined);
                const result = await state.act(
                  () =>
                    state.call('memory.vault.repair.apply', { vault: request, preview_id: token }),
                  '修复已应用',
                );
                if (result) await state.reload();
              })()
            }
          >
            应用此预览
          </button>
          <button type="button" disabled={state.busy} onClick={() => setPreview(undefined)}>
            取消修复
          </button>
        </div>
      ) : null}
      <details>
        <summary>最近修改（{vault.recent_changes.length}）</summary>
        {vault.recent_changes.length ? (
          <ul>
            {vault.recent_changes.map((change, index) => (
              <li key={`${change.at}-${index}`}>
                <time dateTime={change.at}>{new Date(change.at).toLocaleString()}</time> ·{' '}
                {change.title || '（无标题）'} · {ACTIONS[change.action] ?? change.action} ·{' '}
                {ACTORS[change.actor] ?? change.actor}
                <br />
                <code>{change.path}</code>
                {change.session_id ? (
                  <>
                    {' '}
                    · 会话 <code>{change.session_id}</code>
                  </>
                ) : null}
                <br />
                哈希{' '}
                <code>
                  {change.before_hash ?? '无'} → {change.after_hash ?? '无'}
                </code>
              </li>
            ))}
          </ul>
        ) : (
          <p>暂无修改记录。</p>
        )}
      </details>
    </section>
  );
}
