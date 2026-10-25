import { TerminalHost } from './TerminalHost';
import { useToolTabs } from '../right-workspace/tool-tabs';

/** Tests: the conversation's terminal tabs as the right sidebar shows them. */
export function HostHarness({ session }: { session: string }) {
  const state = useToolTabs(session);
  return <TerminalHost sessionId={session} tabs={state.tabs} activeTab={state.active} visible />;
}
