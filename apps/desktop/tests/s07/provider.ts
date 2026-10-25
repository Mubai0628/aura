import assert from 'node:assert/strict';
import {
  writeProvider,
  type ProviderAction,
  type ProviderRequest,
} from '../s06-file-write-provider.mjs';
import { throughEntry } from '../tool-entry';

/** Drive the real discovery and session loading tools before the requested network tool. */
export function networkProvider(onToolReturn?: (request: ProviderRequest) => void) {
  const phases = new Map<string, 'searched' | 'loaded' | 'called'>();
  return writeProvider({
    path: 'unused.txt',
    text: '',
    planAction(request, action): ProviderAction {
      const phase = phases.get(request.turn);
      if (phase === 'called') {
        onToolReturn?.(request);
        return { tool: null, arguments: {} };
      }
      assert.ok(action.tool, 'the network scenario declares its requested tool');
      if (phase === 'searched') {
        const page = lastToolValue(request) as {
          results: Array<{ canonical_name: string; tool_id: string }>;
        };
        const hit = page.results.find((result) => result.canonical_name === action.tool);
        assert.ok(hit, `the real registry must contain ${action.tool}`);
        phases.set(request.turn, 'loaded');
        return { tool: 'tools.load', arguments: { tool_ids: [hit.tool_id], replace: [] } };
      }
      const prefix = `${action.tool.replaceAll('.', '_')}__`;
      const visible = request.tools.some((tool) =>
        (tool as { function: { name: string } }).function.name.startsWith(prefix),
      );
      if (!visible && phase === 'loaded') {
        phases.set(request.turn, 'called');
        return throughEntry(action);
      }
      if (!visible) {
        phases.set(request.turn, 'searched');
        return {
          tool: 'tools.search',
          arguments: { query: action.tool, limit: 50, cursor: null },
        };
      }
      phases.set(request.turn, 'called');
      return action;
    },
  });
}

export function lastToolValue(request: ProviderRequest): Record<string, unknown> {
  const message = request.messages
    .filter((value) => (value as { role?: string }).role === 'tool')
    .at(-1) as { content: string } | undefined;
  assert.ok(message, 'Core sends the durable result in the next model request');
  const result = JSON.parse(message.content) as {
    outcome: { type: string; payload: { type: string; value: Record<string, unknown> } };
  };
  assert.equal(result.outcome.type, 'succeeded', 'the real discovery or network call succeeded');
  assert.equal(result.outcome.payload.type, 'structured_json');
  return result.outcome.payload.value;
}
