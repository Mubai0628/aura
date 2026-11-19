import type { ExecutionApprovalView } from '@aura/contracts/generated';
import { approvalActionLabel } from './approval-action-label';

export function BrowserApprovalFacts({
  view,
  from,
}: {
  view: ExecutionApprovalView;
  from: string;
}) {
  const { action, action_kind: tool, resources } = view.operation.content;
  if (
    !['browser.network', 'browser.inspect', 'browser.performance', 'browser.evaluate'].includes(
      tool,
    )
  )
    return null;
  const page = resources.find((resource) => resource.scope_kind === 'browser_page');
  const site = resources.find((resource) => resource.scope_kind === 'browser_site');
  const binding = (action as { binding?: { origin?: string } }).binding;
  return (
    <dl className="agent-approval-external">
      <dt>请求者</dt>
      <dd>{from}</dd>
      <dt>网站</dt>
      <dd>{binding?.origin ?? site?.host ?? '本地页面'}</dd>
      <dt>页面</dt>
      <dd>
        {page?.page_ref.endsWith('/model')
          ? '内置浏览器的模型页面'
          : page?.page_ref.split('/').at(-1)}
      </dd>
      <dt>请求能力</dt>
      <dd>{approvalActionLabel(tool)}（仅此一次）</dd>
    </dl>
  );
}
