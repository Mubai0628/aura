import { useSyncExternalStore } from 'react';
import { toolTabs } from '../right-workspace/tool-tabs';
import { subscribeTerminals, terminalSnapshot } from '../terminal/store';

/**
 * The model's terminal becomes a tab of the right sidebar only when the user
 * asks to see it — from 进程, or here, under the call in the conversation that opened it.
 */
export function ModelTerminalLink({ callId }: { callId: string }) {
  const records = useSyncExternalStore(subscribeTerminals, terminalSnapshot);
  const record = records.find(
    (item) => item.view.owner === 'agent' && item.view.tool_call_id === callId,
  );
  if (!record) return null;
  const { session_id: session, terminal_id: terminal } = record.view;
  return (
    <p className="agent-tool-terminal">
      <button onClick={() => toolTabs.revealTerminal(session, terminal)} type="button">
        查看终端
      </button>
    </p>
  );
}
