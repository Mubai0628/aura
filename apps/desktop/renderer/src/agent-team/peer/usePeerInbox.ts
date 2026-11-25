import type {
  EventId,
  PeerMailbox,
  PeerMailboxView,
  PeerManageAction,
} from '@aura/contracts/generated';
import { useEffect, useRef, useState } from 'react';

export function usePeerInbox(sessionId: string | null, visible: boolean, view: PeerMailboxView) {
  const api = window.aura?.v1;
  const [value, setValue] = useState<PeerMailbox>();
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [reconciling, setReconciling] = useState(false);
  const scope = useRef<
    | {
        live: boolean;
        reading: boolean;
        writing: boolean;
        uncertain: boolean;
        older: boolean;
        revision: number;
      }
    | undefined
  >(undefined);
  async function refresh(before: EventId | null = null) {
    const owner = scope.current;
    if (!owner?.live || owner.reading || owner.writing || !api?.readPeerMailbox) return;
    owner.reading = true;
    const revision = owner.revision;
    try {
      const result = await api.readPeerMailbox({ session_id: sessionId, before, view });
      if (!owner.live || owner.revision !== revision) return;
      if (!result.ok) throw new Error('mailbox_read');
      owner.older = before !== null;
      owner.uncertain = false;
      setReconciling(false);
      setValue(result.value);
      setError('');
    } catch {
      if (owner.live && owner.revision === revision) setError('暂时无法读取同级协作，请重试。');
    } finally {
      owner.reading = false;
    }
  }
  useEffect(() => {
    const owner = {
      live: visible,
      reading: false,
      writing: false,
      uncertain: false,
      older: false,
      revision: 0,
    };
    scope.current = owner;
    if (!visible || !api?.readPeerMailbox) return;
    const initial = window.setTimeout(() => void refresh(), 0);
    const timer = window.setInterval(() => {
      if (view === 'active' && !owner.older && !document.hidden) void refresh();
    }, 3_000);
    return () => {
      owner.live = false;
      window.clearTimeout(initial);
      window.clearInterval(timer);
    };
    // Every new owner has a fixed session and API for its entire lifetime.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, visible, api, view]);

  async function change(action: PeerManageAction) {
    const owner = scope.current;
    if (!owner?.live || owner.writing || owner.uncertain || !api?.managePeerMailbox) return false;
    owner.writing = true;
    owner.revision++;
    setBusy(true);
    try {
      const result = await api.managePeerMailbox({ session_id: sessionId, action, view });
      if (!owner.live) return false;
      if (!result.ok) throw new Error('mailbox_manage');
      owner.older = false;
      setValue(result.value);
      setError('');
      return true;
    } catch {
      if (owner.live) {
        owner.uncertain = true;
        setReconciling(true);
        setError('操作结果待核对，请先重新读取。');
      }
      return false;
    } finally {
      owner.writing = false;
      if (owner.live) setBusy(false);
    }
  }
  return {
    supported: Boolean(api?.readPeerMailbox),
    value,
    error,
    busy: busy || reconciling,
    refresh,
    change,
    older: () => (value?.before ? refresh(value.before) : Promise.resolve()),
  };
}
