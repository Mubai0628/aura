import type { ExecutionApprovalView } from '@aura/contracts/generated';

export function GitRemoteApprovalFacts({ view }: { view: ExecutionApprovalView }) {
  const content = view.operation.content;
  const resource = content.resources.find((item) => item.scope_kind === 'git_remote_destination');
  if (resource?.scope_kind !== 'git_remote_destination')
    return <p role="alert">远端批准信息缺失，请重新发起操作。</p>;
  const facts = resource.facts;
  const push = content.action_kind === 'git.push';
  return (
    <div className="agent-approval-mode">
      <p>与在终端运行 git 相同，Aura 不隔离它的文件写入</p>
      <p>使用本机 Git 的 hook、签名、LFS、凭据和网络配置；Git 自己联网，不使用 Aura 代理。</p>
      <dl>
        <dt>远端</dt>
        <dd>
          {facts.remote} · {facts.target.remote_url}
        </dd>
        <dt>分支</dt>
        <dd>
          {facts.local_branch ?? '分离 HEAD'} →{' '}
          {facts.target.remote_branch || '按远端 fetch 配置读取'}
        </dd>
        {push ? (
          <>
            <dt>待推送提交</dt>
            <dd>
              {facts.outgoing_count === null
                ? '尚无上次 fetch 的跟踪记录，数量未知'
                : `${facts.outgoing_count} 个（按最近一次 fetch 计算）`}
            </dd>
          </>
        ) : null}
      </dl>
      {push && facts.outgoing_subjects.length > 0 ? (
        <ul>
          {facts.outgoing_subjects.map((subject, index) => (
            <li key={index}>{subject}</li>
          ))}
        </ul>
      ) : null}
      {facts.force_with_lease ? (
        <p>
          精确 force-with-lease：期望远端提交 <code>{facts.force_with_lease}</code>
          。每次单独批准，不能使用会话批准。
        </p>
      ) : push ? (
        <p>
          「本会话允许」只覆盖这个物理仓库、远端地址和远端分支的普通推送；其他分支及精确租约仍需另行批准。重启和模式切换后保留，可在会话联网批准中撤销。
        </p>
      ) : (
        <p>pull 只做快进；分歧请在终端合并或变基。</p>
      )}
      <p>连接中断时，远端可能已经接收操作；请先查询结果，禁止自动重放。</p>
    </div>
  );
}
