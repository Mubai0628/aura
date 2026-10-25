import { shell } from 'electron';
import type { AgentMainParams, AgentMainResult } from '../shared/generated/routes.js';
import type { CoreSupervisor } from './core-supervisor.js';

type Request = AgentMainParams['agent.mcp.oauth.sign_in'];
type Begun = AgentMainResult['agent.mcp.oauth.sign_in'];

/** The only addresses Main opens for a sign-in: https, or http on this computer. */
export function opensInBrowser(address: string): boolean {
  try {
    const url = new URL(address);
    return (
      url.protocol === 'https:' ||
      (url.protocol === 'http:' && ['127.0.0.1', 'localhost', '[::1]'].includes(url.hostname))
    );
  } catch {
    return false;
  }
}

/**
 * Core prepares an OAuth sign-in and answers with the authorization address; Main
 * checks it and opens it in the system browser. The browser's return goes to Core directly.
 */
export async function signInMcpServer(
  request: Request,
  supervisor: CoreSupervisor,
  open: (address: string) => Promise<void> = (address) => shell.openExternal(address),
): Promise<Begun> {
  const begun = await supervisor.requestAgent('agent.mcp.oauth.sign_in', request);
  if (!opensInBrowser(begun.authorization_url))
    throw new Error('mcp.sign_in_refused: 授权地址不是 https，Aura 没有打开它');
  // A browser that does not open leaves the link in the settings page to copy.
  await open(begun.authorization_url).catch(() => undefined);
  return begun;
}
