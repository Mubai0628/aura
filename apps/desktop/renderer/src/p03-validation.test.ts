import { describe, expect, it } from 'vitest';

import {
  isP03MainCommand,
  isP03MainParams,
  isPrepareEditorLifecycleTransition,
} from '../../shared/p03-validation';
import { parseProjection } from '../../shared/projection-validation';
import { validateAgentResponse } from '../../shared/agent-validation';
import { coreRoute, mainRoute } from '../../shared/generated/routes';

const WORKSPACE_ID = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const SESSION_ID = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const OPERATION_ID = 'wlo_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const RENDERER_ID = 'rnd_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const RESERVATION_ID = 'ers_01ARZ3NDEKTSV4RRFFQ69G5FAV';
const TARGET = { path: 'src/main.rs' };
const HASH = {
  algorithm: 'blake3',
  hex: '0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef',
};

describe('P03 Electron boundary', () => {
  it('binds session renaming to an exact session and previous title without execution fields', () => {
    const request = { session_id: SESSION_ID, expected_title: '', title: '首条消息标题' };
    expect(mainRoute('session.rename')?.auth).toBe('trusted_renderer');
    expect(coreRoute('session.rename')?.auth).toBe('authenticated_main');
    expect(isP03MainParams('session.rename', request)).toBe(true);
    expect(isP03MainParams('session.rename', { ...request, model_profile_id: 'injected' })).toBe(
      false,
    );
    expect(isP03MainParams('session.rename', { ...request, session_id: WORKSPACE_ID })).toBe(false);
    expect(
      isP03MainParams('session.rename', { session_id: SESSION_ID, title: 'missing compare value' }),
    ).toBe(false);
  });
  it('L10.7: archive, restore and delete name exactly one session; removal one workspace', () => {
    for (const command of ['session.archive', 'session.restore', 'session.delete'] as const) {
      expect(mainRoute(command)?.auth).toBe('trusted_renderer');
      expect(coreRoute(command)?.auth).toBe('authenticated_main');
      expect(isP03MainParams(command, { session_id: SESSION_ID })).toBe(true);
      expect(isP03MainParams(command, { session_id: WORKSPACE_ID })).toBe(false);
      expect(isP03MainParams(command, { session_id: SESSION_ID, workspace_id: WORKSPACE_ID })).toBe(
        false,
      );
    }
    // Main issues the deletion confirmation itself; the Renderer cannot bring one.
    expect(mainRoute('workspace.delete')?.auth).toBe('trusted_renderer');
    expect(coreRoute('workspace.delete')).toBeUndefined();
    expect(isP03MainParams('workspace.delete', { workspace_id: WORKSPACE_ID })).toBe(true);
    expect(isP03MainParams('workspace.delete', { workspace_id: SESSION_ID })).toBe(false);
    expect(
      isP03MainParams('workspace.delete', {
        workspace_id: WORKSPACE_ID,
        delete_confirmation_id: 'dcn_01ARZ3NDEKTSV4RRFFQ69G5FAV',
      }),
    ).toBe(false);
  });
  it('keeps the directory picker Main-only and rejects Renderer paths and dialog options', () => {
    const command = 'workspace.select_directory';
    expect(mainRoute(command)?.auth).toBe('trusted_renderer');
    expect(coreRoute(command)).toBeUndefined();
    expect(isP03MainParams(command, {})).toBe(true);
    for (const request of [
      { selected_root: 'C:/other' },
      { properties: ['openFile'] },
      { mode: 'full' },
    ])
      expect(isP03MainParams(command, request)).toBe(false);
    expect(validateAgentResponse({ command, value: { status: 'cancelled' } })).toBe(true);
    expect(
      validateAgentResponse({ command, value: { status: 'cancelled', selected_root: 'C:/other' } }),
    ).toBe(false);
    expect(
      validateAgentResponse({
        command,
        value: { status: 'selected', selected_root: 'C:/project' },
      }),
    ).toBe(false);
  });
  it('admits lifecycle/read commands and only the P04-gated write command set', () => {
    expect(isP03MainCommand('workspace.create')).toBe(true);
    expect(isP03MainCommand('workspace.archive')).toBe(true);
    expect(isP03MainCommand('workspace.buffer.edit')).toBe(true);
    expect(isP03MainCommand('fs.read_text')).toBe(true);
    expect(isP03MainCommand('fs.write_file')).toBe(true);
    expect(isP03MainCommand('fs.apply_patch')).toBe(true);
    expect(isP03MainCommand('fs.delete')).toBe(true);
    expect(isP03MainCommand('workspace.buffer.save')).toBe(true);
    expect(isP03MainCommand('workspace.delete_permanently')).toBe(false);
    expect(isP03MainCommand('workspace.lifecycle.begin')).toBe(false);
    expect(isP03MainCommand('workspace.lifecycle.prepare_delivered')).toBe(false);
    expect(isP03MainCommand('workspace.lifecycle.renderer_unavailable')).toBe(false);
  });

  it('validates every security-relevant field of P04-gated mutations', () => {
    const write = {
      content: 'fn main() {}\n',
      expected: { expected_kind: 'file_hash', hash: HASH },
      session_id: SESSION_ID,
      target: TARGET,
      workspace_id: WORKSPACE_ID,
    };
    expect(isP03MainParams('fs.write_file', write)).toBe(true);
    expect(isP03MainParams('fs.write_file', { ...write, bypass_permission: true })).toBe(false);
    expect(
      isP03MainParams('fs.apply_changeset', {
        operations: [
          {
            expected_hash: HASH,
            operation: 'apply_patch',
            target: TARGET,
            unified_diff: '--- a/src/main.rs\n+++ b/src/main.rs\n',
          },
          {
            expected: { expected_kind: 'absent' },
            operation: 'delete',
            recursive: false,
            target: { path: 'target.tmp' },
          },
        ],
        session_id: SESSION_ID,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(true);
    expect(
      isP03MainParams('fs.delete', {
        expected: { expected_kind: 'directory' },
        recursive: true,
        session_id: SESSION_ID,
        target: { path: '../outside' },
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(false);
    expect(
      isP03MainParams('workspace.buffer.save', {
        expected_buffer_revision: '3',
        path: TARGET.path,
        reservation_epoch: '1',
        reservation_id: RESERVATION_ID,
        session_id: SESSION_ID,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(true);
  });

  it('keeps Renderer identity injection in Main and validates prepare events exactly', () => {
    const ack = {
      buffers: [],
      lifecycle_revision: '1',
      operation_id: OPERATION_ID,
      workspace_id: WORKSPACE_ID,
    };
    expect(isP03MainParams('workspace.lifecycle.ack', ack)).toBe(true);
    expect(
      isP03MainParams('workspace.lifecycle.ack', {
        ...ack,
        renderer_instance_id: RENDERER_ID,
      }),
    ).toBe(false);
    const prepare = {
      deadline_at: '2026-08-29T08:00:00Z',
      lifecycle_revision: '1',
      operation: 'archive',
      operation_id: OPERATION_ID,
      workspace_id: WORKSPACE_ID,
    };
    expect(isPrepareEditorLifecycleTransition(prepare)).toBe(true);
    expect(isPrepareEditorLifecycleTransition({ ...prepare, commit: true })).toBe(false);
  });

  it('enforces the confirmed 100-result maximum', () => {
    const request = {
      limit: 100,
      query: 'main',
      session_id: SESSION_ID,
      workspace_id: WORKSPACE_ID,
    };
    expect(isP03MainParams('fs.find_paths', request)).toBe(true);
    expect(isP03MainParams('fs.find_paths', { ...request, limit: 101 })).toBe(false);
  });

  it('accepts optional read fields and rejects traversal targets', () => {
    expect(
      isP03MainParams('fs.read_text', {
        range: { range_kind: 'full' },
        session_id: SESSION_ID,
        target: TARGET,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(true);
    expect(
      isP03MainParams('fs.search_text', {
        include_generated: false,
        include_ignored: false,
        limit: 100,
        pattern: { case_sensitive: false, kind: 'literal', text: 'Aura' },
        root: '.',
        session_id: SESSION_ID,
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(true);
    expect(
      isP03MainParams('fs.stat', {
        session_id: SESSION_ID,
        target: { path: '../outside.txt' },
        workspace_id: WORKSPACE_ID,
      }),
    ).toBe(false);
  });

  it('validates Workspace Files projections and the canonical Workspace root fields', () => {
    const runtime = 'run_01ARZ3NDEKTSV4RRFFQ69G5FAV';
    expect(
      parseProjection({
        payload: {
          lifecycle_operations: [
            {
              ack_deadline_at: '2026-08-29T08:00:00Z',
              error_code: null,
              lifecycle_revision: '1',
              operation: 'archive',
              operation_id: OPERATION_ID,
              preflight: null,
              renderer_instance_id: RENDERER_ID,
              state: 'preparing',
              workspace_id: WORKSPACE_ID,
            },
          ],
          workspaces: [],
        },
        projection_kind: 'workspace_files',
        projection_scope: { type: 'global' },
        revision: '1',
        runtime_instance_id: runtime,
      })?.projection_kind,
    ).toBe('workspace_files');
    expect(
      parseProjection({
        payload: {
          lifecycle_operations: [
            {
              lifecycle_revision: '1',
              operation: 'archive',
              operation_id: OPERATION_ID,
              renderer_instance_id: RENDERER_ID,
              state: 'preparing',
              workspace_id: WORKSPACE_ID,
              renderer_can_commit: true,
            },
          ],
          workspaces: [],
        },
        projection_kind: 'workspace_files',
        projection_scope: { type: 'global' },
        revision: '1',
        runtime_instance_id: runtime,
      }),
    ).toBeUndefined();
    expect(
      parseProjection({
        payload: {
          sessions: [],
          workspaces: [
            {
              authorization_epoch: '1',
              canonical_root: String.raw`C:\Project`,
              catalog_freshness: 'fresh',
              display_name: 'Project',
              revision: '1',
              root_identity: null,
              selected_root: String.raw`C:\Project`,
              state: 'online',
              workspace_id: WORKSPACE_ID,
            },
          ],
        },
        projection_kind: 'workspace_session',
        projection_scope: { type: 'global' },
        revision: '1',
        runtime_instance_id: runtime,
      })?.projection_kind,
    ).toBe('workspace_session');
    expect(
      parseProjection({
        payload: {
          sessions: [],
          workspaces: [
            {
              display_name: 'obsolete',
              root_path: String.raw`C:\Project`,
              state: 'online',
              workspace_id: WORKSPACE_ID,
            },
          ],
        },
        projection_kind: 'workspace_session',
        revision: '1',
        runtime_instance_id: runtime,
      }),
    ).toBeUndefined();
  });
});
