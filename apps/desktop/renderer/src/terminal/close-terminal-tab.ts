import { userMessage } from '../agent-session/user-message';
import { toolTabs, type ToolTab } from '../right-workspace/tool-tabs';
import { removeTerminal, terminalApi, terminalSnapshot } from './store';
import { disposeTerminalEngine } from './xterm-runtime';

/**
 * Closes a terminal tab. A terminal the user opened ends with its tab (unless Core already stopped
 * it); the model's terminal keeps running and only its tab goes. Answers why it did not close.
 */
export async function closeTerminalTab(session: string, tab: ToolTab): Promise<string | undefined> {
  const record = tab.terminalId
    ? terminalSnapshot().find((item) => item.view.terminal_id === tab.terminalId)
    : undefined;
  if (record && record.view.owner !== 'agent') {
    const api = terminalApi();
    if (api && record.view.end?.reason !== 'core_stopped') {
      const result = await api.close({
        session_id: record.view.session_id,
        terminal_id: record.view.terminal_id,
      });
      if (!result.ok) return userMessage(result.error, '终端没有关闭，请重试。');
    }
    disposeTerminalEngine(record.view.terminal_id);
    removeTerminal(record.view.terminal_id);
  }
  toolTabs.close(session, tab.id);
  return undefined;
}
