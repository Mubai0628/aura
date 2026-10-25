import { describe, expect, it } from 'vitest';
import type { SecurityProjectionChange } from '@aura/contracts/generated';
import { isP04MainParams, parseSecurityProjection } from './p04-validation.js';

describe('settings scopes', () => {
  it('defaults accept only a mode and optimistic revision, never a session target', () => {
    expect(isP04MainParams('permission.get_defaults', {})).toBe(true);
    expect(
      isP04MainParams('permission.set_defaults', { mode: 'read_only', expected_revision: 1 }),
    ).toBe(true);
    expect(
      isP04MainParams('permission.set_defaults', { mode: 'invented', expected_revision: 1 }),
    ).toBe(false);
    expect(
      isP04MainParams('permission.set_defaults', {
        mode: 'full_access',
        expected_revision: 1,
        session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      }),
    ).toBe(false);
  });
  it('retains the originating session in real pending-approval projections', () => {
    const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    const event = {
      projection_kind: 'pending_approval',
      runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      revision: '1',
      payload: {
        revision: 1,
        approvals: [
          {
            id: 'apr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
            session_id: session,
            title: 'Write file',
            model_purpose: null,
            actual_operation_summary: 'Write hello.txt',
            source_summary: 'tool',
            risk_flags: [],
            allowed_decisions: ['allow_once', 'deny'],
            request_revision: 1,
            created_at: '2026-09-22T00:00:00Z',
            release_descriptor: null,
          },
        ],
      },
    };
    expect(parseSecurityProjection(event)).toEqual(event);
    expect(
      parseSecurityProjection({
        ...event,
        payload: {
          ...event.payload,
          approvals: [{ ...event.payload.approvals[0], session_id: 'invalid' }],
        },
      }),
    ).toBeUndefined();
  });
});

describe('live permission transition notification', () => {
  const event: SecurityProjectionChange = {
    projection_kind: 'session_permission',
    runtime_instance_id: 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV',
    revision: '1',
    payload: {
      session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      mode: 'read_only',
      revision: 2,
      active_grant_count: 0,
      transition: 'settled',
    },
  };
  it('keeps the connection valid for every actual coordinated phase', () => {
    for (const transition of ['settled', 'stopping', 'stop_unconfirmed'] as const) {
      const current = { ...event, payload: { ...event.payload, transition } };
      expect(parseSecurityProjection(current)).toEqual(current);
    }
  });
  it('does not infer success from an absent, invented or malformed phase', () => {
    const absent = { ...event.payload } as Record<string, unknown>;
    delete absent.transition;
    for (const payload of [
      absent,
      { ...event.payload, transition: 'ready' },
      { ...event.payload, active_grant_count: 4294967296 },
      { ...event.payload, session_id: 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV' },
      { ...event.payload, network_override: true },
    ]) {
      expect(parseSecurityProjection({ ...event, payload })).toBeUndefined();
    }
  });
});
