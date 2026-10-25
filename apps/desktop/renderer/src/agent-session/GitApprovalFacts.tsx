import type {
  ExecutionApprovalView,
  GitBranchInput,
  GitCommitInput,
} from '@aura/contracts/generated';
import { GitRemoteApprovalFacts } from './GitRemoteApprovalFacts';

export function GitApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  if (['git.sync', 'git.push'].includes(view.operation.content.action_kind))
    return <GitRemoteApprovalFacts view={view} />;
  const commit = view.operation.content.action_kind === 'git.commit';
  const input = view.operation.content.action;
  const selection = commit ? (input as unknown as GitCommitInput) : undefined;
  const branch = commit ? undefined : (input as unknown as GitBranchInput);
  return (
    <div className="agent-approval-mode">
      <p>与在终端运行 git 相同，Aura 不隔离它的文件写入</p>
      <p>保留本机 Git 的 hook、签名和 LFS；不跳过 hook，不强制丢弃本地改动。</p>
      {selection ? (
        <dl>
          <dt>提交说明</dt>
          <dd>
            <pre>{selection.message}</pre>
          </dd>
          <dt>文件选择</dt>
          <dd>
            {selection.staged_only
              ? '仅提交已有暂存内容'
              : (selection.paths ?? []).map((path) => <div key={path}>{path}</div>)}
          </dd>
        </dl>
      ) : branch ? (
        <dl>
          <dt>分支操作</dt>
          <dd>
            {branch.kind === 'create'
              ? branch.switch
                ? '新建并切换分支'
                : '新建分支'
              : '切换本地分支'}
          </dd>
          <dt>分支</dt>
          <dd>{branch.name}</dd>
          {branch.kind === 'create' ? (
            <>
              <dt>起点</dt>
              <dd>{branch.start ?? '当前 HEAD'}</dd>
            </>
          ) : null}
        </dl>
      ) : null}
      <p>批准只覆盖列出的本次 Git 操作，不授予终端或其他 Git 操作的权限。</p>
    </div>
  );
}
