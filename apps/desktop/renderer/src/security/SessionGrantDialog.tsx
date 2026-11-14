import { userMessage } from '../agent-session/user-message';
import { SessionFileGrants } from './SessionFileGrants';
import { SessionMemoryGrants } from './memory-grants/SessionMemoryGrants';
import { SessionNetworkGrants } from './network-grants/SessionNetworkGrants';
import { useEffect, useRef, useState, type ReactNode } from 'react';
import { ModalDialog } from '../ModalDialog';
import { idFromBytes } from '../../../shared/id-encoding';
import { localTime } from '../../../shared/local-time';
import {
  CAPABILITY_TEXT,
  EXTERNAL_CAPABILITY_TEXT,
  GRANT_STATE_CHANGED,
  capabilityList,
  grantsNotRead,
  nativeRevokeFailure,
  revokeFailure,
} from './grant-words';
import { AppIcon } from '../app-icons';
import type {
  ExternalGrantView,
  NativeSessionConsentView,
  RevokeNativeSessionConsentInput,
} from '@aura/contracts/generated';
import type { useSecurityControl } from '../security-control-store';
import './security-ui.css';

type DirectoryGrant = ExternalGrantView;

type DirectoryLoad =
  | { status: 'loading'; sessionId: string }
  | { status: 'error'; sessionId: string; message: string }
  | { status: 'ready'; sessionId: string; grants: DirectoryGrant[] };

/**
 * The reusable Native session consent, loaded from its own Core route.
 *
 * It is deliberately not merged into the directory list: the two authorizations
 * have different scope (a directory versus "this Shell, this session"), different
 * lifetime rules (a term versus until revoked, tightened or Core restarts) and
 * different explanations. Showing them as one list would make the withdrawal
 * buttons look equivalent when they are not.
 */
type NativeConsentLoad =
  | { status: 'loading'; sessionId: string }
  | { status: 'error'; sessionId: string; message: string }
  | { status: 'ready'; sessionId: string; consents: NativeSessionConsentView[] };

function directoryScope(grant: DirectoryGrant): string {
  if (grant.target_kind !== 'directory') return '单个文件';
  return grant.recursive_scope ? '该目录及其下内容' : '该目录及直接子项（不含更深层内容）';
}

function inForce(grant: DirectoryGrant): boolean {
  return grant.availability === 'in_force';
}

function directoryTerm(grant: DirectoryGrant): string {
  const term = grant.term === 'workspace' ? '本工作区' : grant.term === 'turn' ? '本轮' : '本会话';
  return grant.expires_at ? `${term}，至 ${localTime(grant.expires_at)}` : term;
}

function consentShell(shell: 'cmd' | 'powershell'): string {
  return shell === 'cmd' ? 'CMD' : 'PowerShell';
}

/**
 * The Shells one consent authorizes, with the tool the user first confirmed named
 * separately. "Which tool was approved" and "what the approval covers" are different
 * facts, and the management list says both.
 */
function consentShells(consent: NativeSessionConsentView): string {
  const ordered = (['powershell', 'cmd'] as const).filter((shell) =>
    consent.scope.shells.includes(shell),
  );
  const covered =
    ordered.length === 0 ? 'PowerShell 与 CMD' : ordered.map(consentShell).join(' 与 ');
  return `${covered}（首次批准：${consentShell(consent.scope.approved_shell)}）`;
}

function consentAvailabilityLabel(availability: NativeSessionConsentView['availability']): string {
  switch (availability) {
    case 'in_force':
      return '生效中';
    case 'revoked':
      return '已撤销';
    case 'consumed':
      return '已用尽';
    case 'expired':
      return '已失效（权限已变化或已超期）';
    case 'superseded':
      return '已失效（Core 已重启）';
    case 'not_yet_valid':
      return '尚未生效';
    default:
      return '状态未知';
  }
}

export function SessionGrantDialog({
  security,
  sessionId,
  open = true,
  onClose,
  title = '会话授权管理',
  titleId = 'session-grants-title',
  children,
}: {
  security: ReturnType<typeof useSecurityControl>;
  sessionId: string;
  /** The dialog stays mounted so "closed" always closes its native layer. */
  open?: boolean;
  onClose: () => void;
  title?: string;
  /** Names the dialog. Another dialog built on this one takes its own: the conversation's stays
   * mounted while closed, and a shared id would name both after whichever comes first. */
  titleId?: string;
  /** What else the conversation was allowed, listed first: a sub-agent's allowances. */
  children?: ReactNode;
}) {
  const [pending, setPending] = useState<string>();
  const [error, setError] = useState<string>();
  const [reload, setReload] = useState(0);
  const [directories, setDirectories] = useState<DirectoryLoad>({
    status: 'loading',
    sessionId,
  });
  const [consents, setConsents] = useState<NativeConsentLoad>({
    status: 'loading',
    sessionId,
  });
  // The management surface owns the command identity so a retry of the same
  // withdrawal is the same command, not a second one.
  const revokeCommands = useRef(new Map<string, string>());
  if (directories.sessionId !== sessionId) {
    setDirectories({ status: 'loading', sessionId });
  }
  if (consents.sessionId !== sessionId) {
    setConsents({ status: 'loading', sessionId });
  }
  useEffect(() => {
    if (!open) return;
    const session = sessionId;
    let current = true;
    void window.aura.v1
      .listExternalGrants({ session_id: session })
      .then((result) => {
        if (!current) return;
        if (!result.ok) {
          setDirectories({
            status: 'error',
            sessionId: session,
            message: grantsNotRead('目录授权', result.error),
          });
          return;
        }
        const rows = (result.value as DirectoryGrant[]).filter(inForce);
        setDirectories({ status: 'ready', sessionId: session, grants: rows });
      })
      .catch((reason: unknown) => {
        if (!current) return;
        setDirectories({
          status: 'error',
          sessionId: session,
          message: grantsNotRead('目录授权', reason),
        });
      });
    return () => {
      current = false;
    };
  }, [open, sessionId, reload]);
  useEffect(() => {
    if (!open) return;
    const session = sessionId;
    let current = true;
    void window.aura.v1
      .listNativeSessionConsents({ session_id: session, include_inactive: true })
      .then((result) => {
        if (!current) return;
        if (!result.ok) {
          setConsents({
            status: 'error',
            sessionId: session,
            message: grantsNotRead('Shell 运行批准', result.error),
          });
          return;
        }
        setConsents({
          status: 'ready',
          sessionId: session,
          consents: result.value as NativeSessionConsentView[],
        });
      })
      .catch((reason: unknown) => {
        if (!current) return;
        setConsents({
          status: 'error',
          sessionId: session,
          message: grantsNotRead('Shell 运行批准', reason),
        });
      });
    return () => {
      current = false;
    };
  }, [open, sessionId, reload]);
  // A switch to another conversation must never briefly expose the prior grant list.
  const ready = security.permission?.session_id === sessionId && !security.loading;
  const grants = ready ? security.grants.filter((grant) => grant.revoked_at === null) : [];
  const revokeConsent = async (consent: NativeSessionConsentView) => {
    if (pending !== undefined) return;
    setPending(consent.grant_id);
    setError(undefined);
    try {
      const existing = revokeCommands.current.get(consent.grant_id);
      const request: RevokeNativeSessionConsentInput = {
        session_id: sessionId,
        grant_id: consent.grant_id,
        expected_revision: consent.grant_revision,
        correlation_id: existing ?? idFromBytes('cor', crypto.getRandomValues(new Uint8Array(16))),
      };
      if (existing === undefined)
        revokeCommands.current.set(consent.grant_id, request.correlation_id);
      const result = await window.aura.v1.revokeNativeSessionConsent(request);
      if (!result.ok) {
        const message = nativeRevokeFailure(result.error);
        setError(message);
        if (message === GRANT_STATE_CHANGED) {
          // The grant moved on, so a press after reading it again is a new command.
          revokeCommands.current.delete(consent.grant_id);
          setConsents({ status: 'loading', sessionId });
          setReload((value) => value + 1);
        }
      } else {
        revokeCommands.current.delete(consent.grant_id);
        setConsents({ status: 'loading', sessionId });
        setReload((value) => value + 1);
      }
    } catch (reason) {
      setError(nativeRevokeFailure(reason));
    } finally {
      setPending(undefined);
    }
  };
  const revokeDirectory = async (id: string) => {
    if (pending !== undefined) return;
    setPending(id);
    setError(undefined);
    try {
      const result = await window.aura.v1.revokeExternalGrant({
        external_grant_id: id,
        session_id: sessionId,
      });
      if (!result.ok) setError(revokeFailure(result.error, '目录授权', '这个目录仍可访问'));
    } catch (reason) {
      setError(revokeFailure(reason, '目录授权', '这个目录仍可访问'));
    } finally {
      setDirectories({ status: 'loading', sessionId });
      setReload((value) => value + 1);
      setPending(undefined);
    }
  };
  const revoke = async (id: string) => {
    if (pending !== undefined) return;
    setPending(id);
    setError(undefined);
    try {
      await security.revokeGrant(id);
    } catch (reason) {
      setError(userMessage(reason, '撤销未提交。'));
    } finally {
      setPending(undefined);
    }
  };
  return (
    <ModalDialog className="session-grant-dialog" titleId={titleId} open={open} onClose={onClose}>
      <header className="modal-header">
        <div>
          <h2 id={titleId}>{title}</h2>
          <p>撤销工作区授权也会影响该工作区的其他会话。</p>
        </div>
        <button className="icon-button" aria-label="关闭授权管理" type="button" onClick={onClose}>
          <AppIcon name="close" />
        </button>
      </header>
      <div className="session-grant-body">
        {children}
        <SessionFileGrants key={sessionId} sessionId={sessionId} open={open} />
        <SessionNetworkGrants key={`network:${sessionId}`} sessionId={sessionId} open={open} />
        <SessionMemoryGrants key={`memory:${sessionId}`} sessionId={sessionId} open={open} />
        {directories.status === 'error' && directories.sessionId === sessionId ? (
          <p role="alert">
            {directories.message}{' '}
            <button
              type="button"
              onClick={() => {
                setDirectories({ status: 'loading', sessionId });
                setReload((value) => value + 1);
              }}
            >
              重试
            </button>
          </p>
        ) : null}
        {!ready || directories.sessionId !== sessionId || directories.status === 'loading' ? (
          <p role="status">正在读取授权…</p>
        ) : directories.status === 'error' ? null : grants.length === 0 &&
          directories.grants.length === 0 ? (
          <p>当前没有目录或工作区授权。</p>
        ) : (
          <>
            {directories.grants.length === 0 ? null : (
              <ul className="compact-list">
                {directories.grants.map((grant) => (
                  <li key={grant.id}>
                    <span>
                      {grant.display_path} · {directoryScope(grant)} · {directoryTerm(grant)} ·{' '}
                      {capabilityList(grant.capabilities, EXTERNAL_CAPABILITY_TEXT)}
                    </span>
                    <button
                      disabled={pending !== undefined}
                      type="button"
                      onClick={() => void revokeDirectory(grant.id)}
                    >
                      {pending === grant.id ? '正在撤销…' : '撤销目录授权'}
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {grants.length === 0 ? null : (
              <ul className="compact-list">
                {grants.map((grant) => (
                  <li key={grant.id}>
                    <span>{capabilityList(grant.capabilities, CAPABILITY_TEXT)}</span>
                    <button
                      disabled={pending !== undefined}
                      type="button"
                      onClick={() => void revoke(grant.id)}
                    >
                      {pending === grant.id ? '正在撤销…' : '撤销'}
                    </button>
                  </li>
                ))}
              </ul>
            )}
          </>
        )}
        <section className="session-grant-native" aria-label="Shell 运行批准">
          <h3>Shell 运行批准</h3>
          {consents.status === 'error' && consents.sessionId === sessionId ? (
            <p role="alert">
              {consents.message}{' '}
              <button
                type="button"
                onClick={() => {
                  setConsents({ status: 'loading', sessionId });
                  setReload((value) => value + 1);
                }}
              >
                重试
              </button>
            </p>
          ) : consents.sessionId !== sessionId || consents.status === 'loading' ? (
            <p role="status">正在读取 Shell 运行批准…</p>
          ) : consents.status === 'error' ? null : consents.consents.length === 0 ? (
            <p>当前会话没有 Shell 运行批准。</p>
          ) : (
            <ul className="compact-list">
              {consents.consents.map((consent) => (
                <li key={consent.grant_id}>
                  <span>
                    {[
                      consentShells(consent),
                      consentAvailabilityLabel(consent.availability),
                      consent.scope.lifetime === 'session'
                        ? '本会话有效'
                        : '旧版批准，Aura Core 重启或权限设置变化后失效',
                      `批准于 ${localTime(consent.issued_at)}`,
                    ].join(' · ')}
                  </span>
                  {consent.availability === 'in_force' ? (
                    <button
                      disabled={pending !== undefined}
                      type="button"
                      onClick={() => void revokeConsent(consent)}
                    >
                      {pending === consent.grant_id ? '正在撤销…' : '撤销 Shell 运行批准'}
                    </button>
                  ) : null}
                </li>
              ))}
            </ul>
          )}
        </section>
        {error === undefined ? null : <p role="alert">{error}</p>}
      </div>
    </ModalDialog>
  );
}
