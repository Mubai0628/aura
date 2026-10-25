import type {
  ApprovalDecisionKind,
  ApprovalView,
  HostPreparationStatus,
  IpcErrorEnvelope,
  PermissionGrant,
  PermissionMode,
  QuarantineRecordDetails,
  SessionPermissionProjection,
} from '@aura/contracts/generated';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import type { ResultEnvelope } from '../../shared/aura-api';
import { readCoreHealthSnapshot, useCoreHealth } from './core-health-store';

interface SecurityControlSnapshot {
  approvals: ApprovalView[];
  error?: string;
  grants: PermissionGrant[];
  host?: HostPreparationStatus;
  loading: boolean;
  permission?: SessionPermissionProjection;
  quarantine: QuarantineRecordDetails[];
  /** Loads in a row whose reads still failed after their retries; 0 once one read. */
  readFailures: number;
  /** The failure the last load ended with, to tell it from an operation's failure. */
  readError?: string;
}

/** One load: what it read. A list or state whose read failed is left out. */
interface SnapshotRead {
  approvals?: ApprovalView[];
  error?: string;
  grants: PermissionGrant[];
  host?: HostPreparationStatus;
  loading: boolean;
  permission?: SessionPermissionProjection;
  quarantine?: QuarantineRecordDetails[];
  readFailed: boolean;
}

const INITIAL: SecurityControlSnapshot = {
  approvals: [],
  grants: [],
  loading: true,
  quarantine: [],
  readFailures: 0,
};

// Main answers every P04 request with "interface unsupported" until the Core
// handshake has negotiated the security-control-plane capability, and again with
// each new runtime generation while it negotiates. Only a settled `ready` health
// projection means a P04 failure is a genuine "unsupported" answer.
const coreCapabilityNegotiated = (): boolean => {
  const health = readCoreHealthSnapshot();
  return !health.loading && health.projection?.payload.state === 'ready';
};

/**
 * A read that
 * fails while Core is busy with other work is tried again before anything is said, and one that
 * kept failing is tried again in the background until it reads. These routes only read (a
 * conversation without a permission state gets its default once), so trying again is safe.
 */
const READ_RETRY_DELAYS_MS = [250, 750] as const;
/** The first read again in the background; each that fails again waits twice as long, up to a minute. */
const BACKGROUND_RETRY_MS = 5_000;
const BACKGROUND_RETRY_LIMIT_MS = 60_000;

const pause = (milliseconds: number) =>
  new Promise<void>((resolve) => {
    window.setTimeout(resolve, milliseconds);
  });

async function retried<T>(
  read: () => Promise<ResultEnvelope<T>>,
  wanted: () => boolean,
): Promise<ResultEnvelope<T>> {
  let result = await read();
  for (const delay of READ_RETRY_DELAYS_MS) {
    // Before the handshake a refusal is the expected "not yet"; the recovery path reloads.
    if (result.ok || !wanted() || !coreCapabilityNegotiated()) return result;
    await pause(delay);
    if (!wanted()) return result;
    result = await read();
  }
  return result;
}

export const useSecurityControl = (workspaceId?: string, sessionId?: string) => {
  const coreHealth = useCoreHealth();
  const coreReady = !coreHealth.loading && coreHealth.projection?.payload.state === 'ready';
  const coreRuntimeId = coreHealth.projection?.runtime_instance_id;
  const [snapshot, setSnapshot] = useState<SecurityControlSnapshot>(INITIAL);
  const [hostPreparationError, setHostPreparationError] = useState<string>();
  const [modeChanging, setModeChanging] = useState(false);
  const modeRequestPending = useRef(false);
  const [fullAccessRequest, setFullAccessRequest] = useState<SessionPermissionProjection>();
  const scope = useMemo(() => ({ sessionId, workspaceId }), [sessionId, workspaceId]);
  const [loadedScope, setLoadedScope] = useState<typeof scope>();
  const activeScope = useRef<typeof scope | undefined>(scope);
  useEffect(() => {
    activeScope.current = scope;
    return () => {
      activeScope.current = undefined;
    };
  }, [scope]);

  const loadSnapshot = useCallback(async (): Promise<SnapshotRead> => {
    const api = window.aura.v1;
    const wanted = () => activeScope.current === scope;
    const [approvals, host, quarantine, permission, grants] = await Promise.all([
      retried(() => api.listPendingApprovals(), wanted),
      retried(() => api.getHostPreparationStatus(), wanted),
      retried(() => api.listQuarantine(), wanted),
      sessionId === undefined
        ? undefined
        : retried(() => api.getSessionPermissionState({ session_id: sessionId }), wanted),
      workspaceId === undefined
        ? undefined
        : retried(
            () =>
              api.listPermissionGrants({
                session_id: sessionId ?? null,
                workspace_id: workspaceId,
              }),
            wanted,
          ),
    ]);
    const results = [approvals, host, quarantine, permission, grants].filter(
      (result) => result !== undefined,
    );
    const failed = results.find((result) => result.ok === false);
    // While the Core handshake (or a new runtime generation's negotiation) is still
    // in progress, a P04 rejection is the transient "not ready" answer, not a real
    // failure: stay in the waiting state instead of surfacing an error banner.
    const waitingForCore = failed?.ok === false && !coreCapabilityNegotiated();
    return {
      ...(approvals.ok ? { approvals: approvals.value } : {}),
      ...(failed?.ok === false && !waitingForCore ? { error: failed.error.message } : {}),
      // Grants belong to this conversation and Workspace: never another scope's.
      grants: grants?.ok === true ? grants.value : [],
      ...(host.ok ? { host: host.value } : {}),
      loading: waitingForCore,
      ...(permission?.ok === true ? { permission: permission.value } : {}),
      ...(quarantine.ok ? { quarantine: quarantine.value } : {}),
      readFailed: failed?.ok === false && !waitingForCore,
    };
  }, [scope, sessionId, workspaceId]);

  const refresh = useCallback(async (): Promise<void> => {
    if (activeScope.current !== scope) return;
    // A notice stays until the load says otherwise: no flicker while a failed read is retried.
    setSnapshot((current) => ({ ...current, loading: true }));
    try {
      const next = await loadSnapshot();
      if (activeScope.current === scope) {
        setSnapshot((current) => mergeSnapshot(current, next));
        setLoadedScope(scope);
      }
    } catch (error) {
      if (activeScope.current !== scope) return;
      setSnapshot((current) => ({
        ...current,
        error: errorMessage(error),
        loading: false,
        readFailures: current.readFailures + 1,
        readError: errorMessage(error),
      }));
    }
  }, [loadSnapshot, scope]);

  useEffect(() => {
    let active = true;
    void loadSnapshot().then(
      (next) => {
        if (active) {
          setSnapshot((current) => mergeSnapshot(current, next));
          setLoadedScope(scope);
        }
      },
      (error: unknown) => {
        if (active) {
          setSnapshot((current) => ({
            ...current,
            error: errorMessage(error),
            loading: false,
            readFailures: current.readFailures + 1,
            readError: errorMessage(error),
          }));
        }
      },
    );
    return () => {
      active = false;
    };
  }, [loadSnapshot, scope]);

  // Retry once capability negotiation settles (Core ready, or a new runtime
  // generation reached ready) so a startup-suppressed failure is re-evaluated
  // against the negotiated capability set and any stale startup error clears.
  const coreRecoveryPending = useRef(false);
  useEffect(() => {
    if (!coreReady) {
      coreRecoveryPending.current = true;
      return;
    }
    if (!coreRecoveryPending.current) return;
    coreRecoveryPending.current = false;
    void refresh();
  }, [coreReady, coreRuntimeId, refresh]);

  // Reads that kept failing are tried again until they read; their notice then goes.
  useEffect(() => {
    if (snapshot.readFailures === 0) return;
    const delay = Math.min(
      BACKGROUND_RETRY_MS * 2 ** (snapshot.readFailures - 1),
      BACKGROUND_RETRY_LIMIT_MS,
    );
    const timer = window.setTimeout(() => void refresh(), delay);
    return () => window.clearTimeout(timer);
  }, [refresh, snapshot.readFailures]);

  useEffect(
    () =>
      window.aura.v1.onSecurityControl((change) => {
        if (change.projection_kind === 'pending_approval') {
          setSnapshot((current) => ({ ...current, approvals: change.payload.approvals }));
          return;
        }
        if (
          change.projection_kind === 'session_permission' &&
          change.payload.session_id === sessionId
        ) {
          setSnapshot((current) => ({
            ...current,
            permission: newerPermission(current.permission, change.payload),
          }));
          return;
        }
        if (change.projection_kind === 'security_runtime') {
          void refresh();
        }
      }),
    [refresh, sessionId],
  );

  const applyMode = useCallback(
    async (mode: PermissionMode, current: SessionPermissionProjection): Promise<void> => {
      if (
        activeScope.current !== scope ||
        current.session_id !== sessionId ||
        modeRequestPending.current
      )
        return;
      modeRequestPending.current = true;
      setModeChanging(true);
      try {
        const result = await window.aura.v1.setSessionPermissionMode({
          expected_revision: current.revision,
          mode,
          session_id: current.session_id,
        });
        if (activeScope.current !== scope) return;
        if (result.ok) {
          setSnapshot((state) =>
            state.permission?.session_id === current.session_id
              ? {
                  ...withoutError(state),
                  permission: newerPermission(state.permission, result.value),
                }
              : state,
          );
        } else {
          // An uncertain reply can follow a committed mode change. Reload the
          // Core authority instead of restoring a stale, more permissive value.
          await refresh();
          setSnapshot((state) =>
            state.permission?.session_id === current.session_id
              ? { ...state, error: result.error.message }
              : state,
          );
        }
      } catch (error) {
        if (activeScope.current !== scope) return;
        await refresh();
        setSnapshot((state) =>
          state.permission?.session_id === current.session_id
            ? { ...state, error: errorMessage(error) }
            : state,
        );
      } finally {
        modeRequestPending.current = false;
        setModeChanging(false);
      }
    },
    [refresh, scope, sessionId],
  );

  const setMode = useCallback(
    async (mode: PermissionMode): Promise<void> => {
      const current = snapshot.permission;
      if (
        current === undefined ||
        modeChanging ||
        (current.mode === mode && (current.transition ?? 'settled') === 'settled')
      )
        return;
      if (mode === 'full_access' && current.mode !== mode) {
        setFullAccessRequest(current);
        return;
      }
      await applyMode(mode, current);
    },
    [applyMode, modeChanging, snapshot.permission],
  );

  const confirmFullAccess = useCallback(async (): Promise<void> => {
    if (fullAccessRequest?.session_id !== sessionId || fullAccessRequest === undefined) return;
    await applyMode('full_access', fullAccessRequest);
    setFullAccessRequest(undefined);
  }, [applyMode, fullAccessRequest, sessionId]);

  const resolveApproval = useCallback(
    async (approval: ApprovalView, decision: ApprovalDecisionKind): Promise<void> => {
      const result = await window.aura.v1.resolveApproval({
        approval_id: approval.id,
        decision,
        expected_request_revision: approval.request_revision,
      });
      if (result.ok) await refresh();
      else setSnapshot((state) => ({ ...state, error: result.error.message }));
    },
    [refresh],
  );

  const revokeGrant = useCallback(async (grantId: string): Promise<void> => {
    const result = await window.aura.v1.revokePermissionGrant({ grant_id: grantId });
    if (result.ok) {
      setSnapshot((state) => ({
        ...withoutError(state),
        grants: state.grants.map((grant) => (grant.id === result.value.id ? result.value : grant)),
      }));
    } else {
      setSnapshot((state) => ({ ...state, error: result.error.message }));
    }
  }, []);

  const requestHostPreparation = useCallback(
    async (action: 'prepare' | 'repair' | 'remove'): Promise<void> => {
      const host = snapshot.host;
      if (host === undefined || host.active_operation !== null) return;
      setHostPreparationError(undefined);
      const request = { expected_revision: host.revision };
      const result =
        action === 'prepare'
          ? await window.aura.v1.prepareHostPreparation(request)
          : action === 'repair'
            ? await window.aura.v1.repairHostPreparation(request)
            : await window.aura.v1.removeHostPreparation(request);
      if (result.ok) {
        setSnapshot((state) => ({ ...withoutError(state), host: result.value }));
      } else {
        setHostPreparationError(hostPreparationErrorMessage(result.error));
        await refresh();
      }
    },
    [refresh, snapshot.host],
  );

  const { readFailures, readError, ...state } = snapshot;
  void readFailures;
  return {
    ...state,
    /** The failure is a read that kept failing, not an operation the user started. */
    readFailed: snapshot.error !== undefined && snapshot.error === readError,
    loading: snapshot.loading || loadedScope !== scope,
    grants: loadedScope === scope ? snapshot.grants : [],
    permission: snapshot.permission?.session_id === sessionId ? snapshot.permission : undefined,
    ...(snapshot.error === undefined && hostPreparationError !== undefined
      ? { error: hostPreparationError }
      : {}),
    refresh,
    resolveApproval,
    requestHostPreparation,
    revokeGrant,
    setMode,
    modeChanging,
    fullAccessConfirmation:
      fullAccessRequest !== undefined && fullAccessRequest.session_id === sessionId,
    confirmFullAccess,
    cancelFullAccess: () => setFullAccessRequest(undefined),
  };
};

const newerPermission = (
  current: SessionPermissionProjection | undefined,
  next: SessionPermissionProjection,
): SessionPermissionProjection => {
  if (current?.session_id !== next.session_id) return next;
  if (
    current.revision > next.revision ||
    (current.revision === next.revision &&
      (current.transition ?? 'settled') === 'settled' &&
      (next.transition ?? 'settled') !== 'settled')
  )
    return current;
  return next;
};
/** A read that failed keeps what was known before it: the lists, Host preparation, the mode. */
const mergeSnapshot = (
  current: SecurityControlSnapshot,
  next: SnapshotRead,
): SecurityControlSnapshot => {
  const host = next.host ?? current.host;
  const permission = next.permission
    ? newerPermission(current.permission, next.permission)
    : current.permission;
  return {
    approvals: next.approvals ?? current.approvals,
    ...(next.error === undefined ? {} : { error: next.error }),
    grants: next.grants,
    ...(host ? { host } : {}),
    loading: next.loading,
    ...(permission ? { permission } : {}),
    quarantine: next.quarantine ?? current.quarantine,
    readFailures: next.readFailed ? current.readFailures + 1 : 0,
    ...(next.readFailed && next.error !== undefined ? { readError: next.error } : {}),
  };
};

const withoutError = (snapshot: SecurityControlSnapshot): SecurityControlSnapshot => {
  const { error, ...current } = snapshot;
  void error;
  return current;
};

const errorMessage = (error: unknown): string =>
  error instanceof Error ? error.message : '安全控制面刷新失败。';

const hostPreparationErrorMessage = (error: IpcErrorEnvelope): string =>
  error.code === 'sandbox.host_preparation_release_trust_failed'
    ? '当前 Release Bundle 未通过 Aura Production Root 信任验证；Aura 已按 fail-closed 阻止管理员权限请求，Development/Test Bundle 不会回退到其他信任根。'
    : error.message;
