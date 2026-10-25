import assert from 'node:assert/strict';
import { test } from 'node:test';
import { answerApproval } from './approvals.mjs';

test('fixed IPC answers permit requested Shell running and deny network or outside file scope', async () => {
  const page = { evaluate: (callback, args) => callback(args) };
  const decisions = [];
  let capabilities = ['process_execute'];
  let resources = [{ scope_kind: 'system_path' }];
  globalThis.window = {
    aura: {
      v1: {
        getAgentApproval: async () => ({
          ok: true,
          value: {
            approval: { revision: '2' },
            requested_capabilities: capabilities,
            operation: { content: { capabilities, resources } },
          },
        }),
        resolveAgentApproval: async (value) => {
          decisions.push(value);
          return { ok: true };
        },
      },
    },
  };
  try {
    assert.equal(
      (await answerApproval(page, 'session', 'run', 'allow_once')).decision,
      'allow_once',
      'trusted Shell image does not constitute external file access',
    );
    capabilities = ['process_execute', 'ordinary_network'];
    assert.equal((await answerApproval(page, 'session', 'network', 'allow_once')).decision, 'deny');
    capabilities = ['external_file_write'];
    resources = [{ scope_kind: 'external_path' }];
    assert.equal((await answerApproval(page, 'session', 'outside', 'allow_once')).decision, 'deny');
    assert.ok(
      decisions.every((r) => r.expected_revision === '2'),
      'actual immutable approval revision is submitted',
    );
  } finally {
    delete globalThis.window;
  }
});
