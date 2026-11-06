import type { MessageReferenceView } from '@aura/contracts/generated';
import { referenceText } from './reference-text';
import './composer-references.css';

/** What a message referred to when it was sent, once each. */
export function MessageReferencesLine({
  references,
}: {
  references: readonly MessageReferenceView[] | undefined;
}) {
  const shown = [...new Set((references ?? []).map(referenceText))];
  return shown.length ? <p className="message-references-line">引用：{shown.join('、')}</p> : null;
}
