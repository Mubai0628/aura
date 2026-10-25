import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, test, vi } from 'vitest';
import type { SessionToolActivityProjection } from '@aura/contracts/generated';

import { SessionToolActivityCard } from './SessionToolActivityCard';

const readToolActivityPage = vi.fn();

beforeEach(() => {
  readToolActivityPage.mockReset();
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readToolActivityPage } },
  });
});

afterEach(cleanup);

const emptyActivity: SessionToolActivityProjection = {
  active_batches: [],
  older_activity_cursor: null,
  recent_calls: [],
  session_id: 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV',
};

describe('SessionToolActivityCard', () => {
  test('does not manufacture a card when Core has no activity', () => {
    const { container } = render(<SessionToolActivityCard activity={emptyActivity} />);

    expect(container.firstChild).toBeNull();
  });

  test('renders a renderer-safe terminal call from the committed projection', () => {
    render(
      <SessionToolActivityCard
        activity={{
          ...emptyActivity,
          older_activity_cursor: 'opaque-cursor',
          recent_calls: [
            {
              can_cancel: false,
              completed_at: '2026-09-05T08:30:00Z',
              display_name: 'Fixture Echo',
              ordinal: 0,
              outcome: { type: 'succeeded' },
              side_effect_state: 'no_effect',
              started_at: '2026-09-05T08:29:59Z',
              status: 'finalized',
              tool_batch_id: 'tbt_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              tool_version: { definition_revision: '1', semantic_version: '1.0.0' },
            },
          ],
        }}
      />,
    );

    expect(screen.getByRole('region', { name: 'Tool 活动' })).toBeTruthy();
    expect(screen.getByText('Fixture Echo')).toBeTruthy();
    expect(screen.getByText('调用已完成')).toBeTruthy();
    expect(screen.getByRole('button', { name: '加载更早活动' })).toBeTruthy();
  });

  test('loads the exact opaque cursor and appends terminal history', async () => {
    readToolActivityPage.mockResolvedValue({
      ok: true,
      value: {
        cutoff_session_sequence: 42,
        items: [
          {
            artifact_unavailable: true,
            call: {
              can_cancel: false,
              completed_at: '2026-09-05T08:20:00Z',
              display_name: 'Historical Tool',
              ordinal: 1,
              outcome: { type: 'failed' },
              side_effect_state: 'unknown',
              started_at: '2026-09-05T08:19:59Z',
              status: 'finalized',
              tool_batch_id: 'tbt_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              tool_call_id: 'tcl_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              tool_version: { definition_revision: '2', semantic_version: null },
            },
            definition_ref: {
              registry_snapshot_ref: {
                content_hash: {
                  algorithm: 'sha256',
                  canonicalization_version: 'rfc8785_jcs_v1',
                  hex: '11'.repeat(32),
                },
                registry_revision: '7',
                tool_registry_snapshot_id: 'trs_01ARZ3NDEKTSV4RRFFQ69G5FAV',
              },
              tool_id: 'tol_01ARZ3NDEKTSV4RRFFQ69G5FAW',
              tool_version: { definition_revision: '2', semantic_version: null },
            },
            session_sequence: 41,
            source: 'imported',
            tool_instance_id: 'tin_01ARZ3NDEKTSV4RRFFQ69G5FAV',
          },
        ],
        next_cursor: null,
        session_id: emptyActivity.session_id,
      },
    });
    render(
      <SessionToolActivityCard
        activity={{
          ...emptyActivity,
          older_activity_cursor: 'opaque-cursor',
          recent_calls: [],
          active_batches: [
            {
              call_count: 1,
              can_cancel: false,
              completed_at: null,
              completed_call_count: 0,
              started_at: '2026-09-05T08:30:00Z',
              status: 'running',
              tool_batch_id: 'tbt_01ARZ3NDEKTSV4RRFFQ69G5FAV',
            },
          ],
        }}
      />,
    );

    fireEvent.click(screen.getByRole('button', { name: '加载更早活动' }));

    await waitFor(() => expect(screen.getByText('Historical Tool')).toBeTruthy());
    expect(readToolActivityPage).toHaveBeenCalledWith({ cursor: 'opaque-cursor' });
    expect(screen.queryByRole('button', { name: '加载更早活动' })).toBeNull();
  });
});
