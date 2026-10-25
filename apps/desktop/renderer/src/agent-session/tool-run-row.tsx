import type { AgentToolActivityEntry } from '@aura/contracts/generated';
import { ExpandIcon, ToolIcon } from '../tool-icon';
import { toolStateIcon, toolTypeIcon } from '../tool-icon-map';
import { ToolActivity } from './ToolActivity';
import type { ToolCallGroup } from './tool-call-group';
import { useDisclosureOpen } from './disclosure-open';

/** A run of identical Tool calls in one final state: one collapsed row that
 *  keeps every member, with the same row layout as a single call. */
export function ToolCallRun({
  group,
  calls,
}: {
  group: ToolCallGroup;
  calls: ReadonlyMap<string, AgentToolActivityEntry>;
}) {
  const { onToggle, open } = useDisclosureOpen();
  const targets = group.callIds
    .flatMap((id) => {
      const target = calls.get(id)?.display.target;
      return typeof target === 'string' && target.length > 0 ? [{ id, target }] : [];
    })
    .slice(0, 4);
  return (
    <details className="agent-tool-group" onToggle={onToggle}>
      <summary className="agent-tool-row">
        <span className="agent-tool-leading">
          <ToolIcon className="agent-tool-kind" name={toolTypeIcon(group.displayName)} />
        </span>
        <span className="agent-tool-main">
          <span className="agent-tool-title" title={group.displayName}>
            {group.title || group.displayName}
          </span>
          <span className="agent-tool-state" data-tone={group.tone}>
            <ToolIcon name={toolStateIcon(group.tone)} size={14} />
            {group.state} · {group.callIds.length} 次
          </span>
          <span className="agent-tool-chevron" aria-hidden="true">
            <ExpandIcon open={open} />
          </span>
          {targets.length > 0 ? (
            <span className="agent-tool-targets">
              {targets.map(({ id, target }) => (
                <span className="agent-tool-target" key={id} title={target}>
                  {target}
                </span>
              ))}
            </span>
          ) : null}
        </span>
      </summary>
      <div className="agent-tool-group-members">
        {group.callIds.map((id) => (
          <ToolActivity key={id} entry={calls.get(id)} />
        ))}
      </div>
    </details>
  );
}
