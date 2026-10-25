import type { ExecutionApprovalView } from '@aura/contracts/generated';

export function MemoryApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const vault = view.operation.content.resources.find((r) => r.scope_kind === 'memory_vault');
  if (!vault || vault.scope_kind !== 'memory_vault') return null;
  const raw = view.operation.content.action;
  const action = raw && typeof raw === 'object' && !Array.isArray(raw) ? raw : {};
  const titles: string[] = Array.isArray(action.titles)
    ? (action.titles as unknown[]).filter((title): title is string => typeof title === 'string')
    : [];
  const deleting = view.operation.content.capabilities.includes('memory_delete');
  return (
    <div aria-label="记忆操作范围">
      <dl>
        <dt>记忆库</dt>
        <dd>{vault.scope.kind === 'global' ? '全局记忆' : '当前项目记忆'}</dd>
        <dt>操作</dt>
        <dd>{deleting ? '永久删除记忆文件' : '写入记忆'}</dd>
        <dt>记忆标题</dt>
        <dd>
          {titles.length
            ? titles.map((title) => title.slice(0, 200)).join('、')
            : '以本次调用选定的记忆为准'}
        </dd>
      </dl>
      <p>
        {deleting
          ? '删除始终需要逐次确认；仅保留哈希和标题的修改记录。'
          : '本会话允许只覆盖此记忆库中的内置记忆写入，不授权普通文件工具。'}
      </p>
    </div>
  );
}
