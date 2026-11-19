import type { BrowserHostActionV1 } from '@aura/contracts/generated';
import type { BrowserPolicy } from './policy.js';
import { decideSite, siteOfUrl } from './site-rules.js';

export function isDebugging(action: BrowserHostActionV1): boolean {
  return ['evaluate', 'network', 'inspect', 'performance'].includes(action.kind);
}

export function debuggingRefusal(
  action: BrowserHostActionV1,
  policy: BrowserPolicy,
  documentId: string | undefined,
  url: string,
): string | undefined {
  if (!isDebugging(action)) return undefined;
  if (!policy.enabled || policy.debuggingBlocked) return '当前浏览器设置禁止网页调试，本次未执行。';
  const site = siteOfUrl(url);
  if (site && decideSite(site, policy.allowed, policy.blocked) === 'blocked')
    return '当前页面已被浏览器策略阻止，本次调试未执行。';
  const origin = site ? new URL(url).origin : undefined;
  const binding = 'binding' in action ? action.binding : undefined;
  if (
    !binding ||
    binding.document_id !== documentId ||
    binding.settings_revision !== policy.revision ||
    (binding.origin ?? undefined) !== origin
  )
    return '审批对应的网页或浏览器权限已变化，本次未执行。请先刷新页面快照。';
  return undefined;
}
