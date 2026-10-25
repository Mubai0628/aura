import { describe, expect, it } from 'vitest';

import { isP04MainCommand, isP04MainParams } from '../../shared/p04-validation';

const WORKSPACE_ID = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SESSION_ID = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const ARTIFACT_ID = 'art_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const EXTERNAL_GRANT_ID = 'xgr_01ARZ3NDEKTSV4RRFFQ69G5FAV';
describe('P04 Electron boundary', () => {
  it('lets Renderer request Host Preparation actions but never resolve an approval', () => {
    expect(isP04MainCommand('security.host_preparation.status')).toBe(true);
    expect(isP04MainCommand('security.host_preparation.probe')).toBe(false);
    expect(isP04MainCommand('security.host_preparation.resolve')).toBe(false);
    expect(isP04MainCommand('security.host_preparation.prepare')).toBe(true);
    expect(isP04MainCommand('security.host_preparation.repair')).toBe(true);
    expect(isP04MainCommand('security.host_preparation.remove')).toBe(true);
  });

  it('accepts only the current Host Preparation revision from Renderer', () => {
    const request = { expected_revision: 3 } as const;
    expect(isP04MainParams('security.host_preparation.prepare', request)).toBe(true);
    expect(isP04MainParams('security.host_preparation.repair', request)).toBe(true);
    expect(isP04MainParams('security.host_preparation.remove', request)).toBe(true);
    expect(
      isP04MainParams('security.host_preparation.prepare', {
        ...request,
        approval_id: 'apr_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      }),
    ).toBe(false);
    expect(isP04MainParams('security.host_preparation.prepare', { expected_revision: -1 })).toBe(
      false,
    );
  });

  it('never accepts a Renderer-selected external canonical path', () => {
    expect(
      isP04MainParams('security.external_grant.attach_read_only', {
        session_id: SESSION_ID,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(true);
    expect(
      isP04MainParams('security.external_grant.attach_read_only', {
        selected_path: String.raw`C:\outside\secret.txt`,
        session_id: SESSION_ID,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(false);
  });

  it('limits ordinary Artifact Export to opaque Workspace or External Grant targets', () => {
    const request = {
      artifact_id: ARTIFACT_ID,
      expected: { expected_kind: 'absent' },
      session_id: SESSION_ID,
      target: {
        external_grant_id: EXTERNAL_GRANT_ID,
        relative_path: 'report.pdf',
        target_kind: 'external_grant',
      },
      workspace_id: WORKSPACE_ID,
    } as const;
    expect(isP04MainParams('artifact.export', request)).toBe(true);
    expect(
      isP04MainParams('artifact.export', {
        ...request,
        target: {
          canonical_path: String.raw`C:\outside\report.pdf`,
          target_kind: 'external_path',
        },
      }),
    ).toBe(false);
  });
});
