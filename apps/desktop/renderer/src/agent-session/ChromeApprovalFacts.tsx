import type { ExecutionApprovalView } from '@aura/contracts/generated';
import type { ChromeFacts } from './chrome-approval-facts';

/** A call in the user's own Chrome: the tab, the site, and what this answer covers. */
export function ChromeApprovalFacts({
  view,
  facts,
}: {
  view: ExecutionApprovalView;
  facts: ChromeFacts;
}) {
  const turn = view.approval.allowed_decisions.includes('allow_for_current_turn');
  return (
    <section aria-label="Chrome 操作">
      <dl className="agent-approval-workspace">
        <dt>标签页</dt>
        <dd>{facts.tab === 'new' ? '新标签页' : facts.tab}</dd>
        {facts.origin ? (
          <>
            <dt>网站</dt>
            <dd>
              <code>{facts.origin}</code>
              {facts.confirmed ? null : '（尚未确认）'}
            </dd>
          </>
        ) : null}
      </dl>
      {turn ? <p>“本轮允许”只覆盖这个共享里的读取和操作，敏感操作仍会每次询问。</p> : null}
    </section>
  );
}
