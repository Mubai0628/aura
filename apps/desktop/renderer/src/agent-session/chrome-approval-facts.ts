import type { ExecutionApprovalView } from '@aura/contracts/generated';

/** What a Chrome call names: the conversation's tab and the site it is on or goes to. */
export interface ChromeFacts {
  tab: string;
  origin: string | null;
  /** The share's user has confirmed this site. */
  confirmed: boolean;
  sensitive: boolean;
}

export function chromeFacts(view: ExecutionApprovalView): ChromeFacts | undefined {
  const content = view.operation.content;
  if (!content.action_kind.startsWith('chrome.')) return undefined;
  let tab = '';
  let origin: string | null = null;
  let confirmed = true;
  for (const resource of content.resources) {
    if (resource.scope_kind === 'chrome_tab') tab = resource.tab_ref;
    if (resource.scope_kind === 'chrome_origin') {
      origin = resource.origin;
      confirmed = resource.confirmed;
    }
  }
  return {
    tab,
    origin,
    confirmed,
    sensitive: content.capabilities.includes('chrome_sensitive_action'),
  };
}

/** Why a Chrome call is asked about, said before anything else on the card. */
export function chromeWarning(facts: ChromeFacts): string | null {
  if (facts.sensitive) return '这一步可能购买、付款、删除、发送、发布或转账；只能允许这一次。';
  if (!facts.confirmed && facts.origin)
    return `${facts.origin} 还没有为这个共享确认过；只能允许这一次，允许后这个共享会记住它。`;
  return null;
}
