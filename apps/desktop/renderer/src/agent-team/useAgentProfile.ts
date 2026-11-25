import { useEffect, useRef, useState } from 'react';
import type { AgentProfile, AgentProfilePreview } from '@aura/contracts/generated';

export function useAgentProfile(sessionId: string, visible: boolean) {
  const [profile, setProfile] = useState<AgentProfile>();
  const [draft, setDraft] = useState('');
  const [problem, setProblem] = useState<string>();
  const [busy, setBusy] = useState(false);
  const [retry, setRetry] = useState(0);
  const dirty = useRef(false);
  const [isDirty, setDirty] = useState(false);
  const editRevision = useRef<number | undefined>(undefined);
  const latest = useRef<AgentProfile | undefined>(undefined);
  const active = useRef(true);
  const api = window.aura?.v1;
  useEffect(() => {
    active.current = true;
    return () => {
      active.current = false;
    };
  }, []);
  useEffect(() => {
    if (!visible || !api?.readAgentProfile) return;
    let alive = true;
    let reading = false;
    const read = async () => {
      if (reading) return;
      reading = true;
      try {
        const result = await api.readAgentProfile({ session_id: sessionId });
        if (!alive) return;
        if (!result.ok) {
          setProblem('简介暂时读不到，请重试；聊天不受影响。');
          return;
        }
        if (latest.current && latest.current.revision > result.value.revision) return;
        latest.current = result.value;
        setProfile(result.value);
        if (!dirty.current) setDraft(result.value.description);
        setProblem(undefined);
      } catch {
        if (alive) setProblem('简介暂时读不到，请重试；聊天不受影响。');
      } finally {
        reading = false;
      }
    };
    void read();
    const timer = window.setInterval(() => void read(), 5000);
    return () => {
      alive = false;
      window.clearInterval(timer);
    };
  }, [api, sessionId, visible, retry]);

  const save = async () => {
    if (!profile || busy) return;
    setBusy(true);
    try {
      const result = await api.setAgentProfile({
        session_id: sessionId,
        expected_revision: editRevision.current ?? profile.revision,
        description: draft,
      });
      if (!active.current) return;
      if (!result.ok) {
        setProblem('保存结果尚未确认，或简介已经更新；请先重新读取，不要重复保存。');
        return;
      }
      dirty.current = false;
      setDirty(false);
      editRevision.current = undefined;
      latest.current = result.value;
      setProfile(result.value);
      setDraft(result.value.description);
      setProblem(undefined);
    } catch {
      if (active.current) setProblem('保存结果尚未确认；请重新读取后检查，聊天不受影响。');
    } finally {
      if (active.current) setBusy(false);
    }
  };
  const preview = async () => {
    if (!profile || busy || dirty.current || !api.previewAgentProfile) return;
    setBusy(true);
    try {
      const result = await api.previewAgentProfile({
        session_id: sessionId,
        expected_revision: profile.revision,
      });
      if (!active.current) return;
      if (!result.ok) {
        setProblem(
          '建议尚未生成或请求结果未确认；请先完成一轮对话并检查会话模型，再重新读取确认。',
        );
        return;
      }
      latest.current = result.value;
      setProfile(result.value);
      setProblem(undefined);
    } catch {
      if (active.current) setProblem('生成请求结果尚未确认；请重新读取，不要重复发起。');
    } finally {
      if (active.current) setBusy(false);
    }
  };
  return {
    profile,
    draft,
    problem,
    busy,
    dirty: isDirty,
    available: Boolean(api?.readAgentProfile),
    canPreview: Boolean(api?.previewAgentProfile),
    save,
    preview,
    edit: (text: string) => {
      if (!dirty.current) editRevision.current = profile?.revision;
      dirty.current = true;
      setDirty(true);
      setDraft(text);
    },
    refresh: () => {
      dirty.current = false;
      setDirty(false);
      editRevision.current = undefined;
      setRetry((value) => value + 1);
    },
    adopt: (candidate: AgentProfilePreview) => {
      if (candidate.base_revision !== profile?.revision) return;
      dirty.current = true;
      setDirty(true);
      editRevision.current = candidate.base_revision;
      setDraft(candidate.description);
    },
  };
}
