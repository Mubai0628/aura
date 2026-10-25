import type { ProviderAction } from './s06-file-write-provider.mjs';

/**
 * A tool call through the unified entry. A load applies from the next turn, so in the turn that
 * loaded a tool the model reaches it through `tools.call`, which admission, approvals and the
 * result treat exactly as the tool's own call.
 */
export const throughEntry = (action: ProviderAction): ProviderAction => ({
  tool: 'tools.call',
  arguments: { tool: action.tool, arguments: action.arguments },
});
