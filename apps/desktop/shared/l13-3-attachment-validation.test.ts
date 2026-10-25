import { expect, test } from 'vitest';
import { parseAgentResult } from './agent-validation.js';

const ulid = '01ARZ3NDEKTSV4RRFFQ69G5FAV';
const external = {
  kind: 'external_file',
  name: 'bundle.zip',
  path: 'C:\\Downloads\\bundle.zip',
  byte_length: 200,
  external_grant_id: `xgr_${ulid}`,
  listing_artifact_id: `art_${ulid}`,
};
const inbox = (attachment: unknown) => ({
  session_id: `ses_${ulid}`,
  revision: '1',
  paused: false,
  pause_reason: null,
  messages: [
    {
      message_id: `evt_${ulid}`,
      revision: '1',
      text: 'look',
      destination: { lane: 'next_turn' },
      created_at: '2026-10-23T12:00:00Z',
      attachments: [attachment],
    },
  ],
});

test('L13.3 generated desktop validator accepts externally granted files and user ZIP lists', () => {
  expect(() => parseAgentResult('agent.inbox', inbox(external))).not.toThrow();
  const locationOnly = { ...external };
  Reflect.deleteProperty(locationOnly, 'listing_artifact_id');
  expect(() => parseAgentResult('agent.inbox', inbox(locationOnly))).not.toThrow();
  expect(() =>
    parseAgentResult(
      'agent.inbox',
      inbox({
        kind: 'file',
        name: 'bundle.zip',
        path: 'bundle.zip',
        byte_length: 200,
        listing_artifact_id: `art_${ulid}`,
      }),
    ),
  ).not.toThrow();
});

test('L13.3 generated validator refuses missing, mistyped and workspace-file authority', () => {
  const missing = { ...external };
  Reflect.deleteProperty(missing, 'external_grant_id');
  expect(() => parseAgentResult('agent.inbox', inbox(missing))).toThrow();
  expect(() =>
    parseAgentResult('agent.inbox', inbox({ ...external, external_grant_id: 17 })),
  ).toThrow();
  expect(() =>
    parseAgentResult('agent.inbox', inbox({ ...external, kind: 'file', path: 'bundle.zip' })),
  ).toThrow();
});

test('L13.3 old workspace attachments remain valid without a listing or grant', () => {
  expect(() =>
    parseAgentResult(
      'agent.inbox',
      inbox({
        kind: 'file',
        name: 'spec.pdf',
        path: 'docs/spec.pdf',
        byte_length: 19,
      }),
    ),
  ).not.toThrow();
});
