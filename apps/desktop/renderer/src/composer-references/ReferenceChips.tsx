import { AppIcon, type IconName } from '../app-icons';
import type { ChosenReference } from './useReferenceMenu';
import './composer-references.css';

const KINDS: Record<ChosenReference['kind'], { icon: IconName; name: string }> = {
  session: { icon: 'chat', name: '会话' },
  terminal: { icon: 'terminal', name: '终端' },
  page: { icon: 'browser', name: '网页' },
  resource: { icon: 'plug', name: 'MCP 资源' },
};

/**
 * The conversations, terminals, pages and MCP resources chosen from the "@" list, sent with the
 * message; a terminal or page is read when it is sent, and one that cannot be read says why.
 */
export function ReferenceChips({
  chosen,
  error,
  onRemove,
}: {
  chosen: readonly ChosenReference[];
  error?: string | undefined;
  onRemove: (key: string) => void;
}) {
  if (!chosen.length) return null;
  return (
    <div className="composer-reference-chips" role="group" aria-label="引用">
      {chosen.map((one) => (
        <span
          className="composer-reference-chip"
          key={one.key}
          title={one.capture?.detail ?? one.resource?.uri}
        >
          <AppIcon name={KINDS[one.kind].icon} size={12} />
          {one.title}
          <button
            type="button"
            aria-label={`移除引用的${KINDS[one.kind].name} ${one.title}`}
            onClick={() => onRemove(one.key)}
          >
            <AppIcon name="close" size={13} />
          </button>
        </span>
      ))}
      {error ? <p role="alert">{error}</p> : null}
    </div>
  );
}
