import { userMessage } from '../agent-session/user-message';
import { useCallback, useEffect, useRef, useState } from 'react';
import type {
  MemoryDesktopSnapshot,
  MemorySettingsSnapshot,
  WorkspaceId,
} from '@aura/contracts/generated';
import type {
  MemoryMainCommand,
  MemoryMainParams,
  MemoryMainResult,
} from '../../../shared/memory-api';
export function useMemory(coreReady: boolean, workspaceId?: WorkspaceId) {
  const [snapshot, setSnapshot] = useState<MemoryDesktopSnapshot>();
  const [settings, setSettings] = useState<MemorySettingsSnapshot>();
  const [error, setError] = useState<string>();
  const [message, setMessage] = useState<string>();
  const [busy, setBusy] = useState(false);
  const generation = useRef({ value: 0 });
  const locked = useRef(false);
  const call = useCallback(
    async <C extends MemoryMainCommand>(
      command: C,
      params: MemoryMainParams[C],
    ): Promise<MemoryMainResult[C]> => {
      if (!coreReady || !window.aura?.v1.memory)
        throw new Error('Core 尚未就绪，请等待连接后重试。');
      const result = await window.aura.v1.memory.invoke(command, params);
      if (!result.ok) throw new Error(result.error.message);
      return result.value;
    },
    [coreReady],
  );
  const reload = useCallback(async () => {
    const current = ++generation.current.value;
    locked.current = false;
    setBusy(false);
    setError(undefined);
    setSnapshot(undefined);
    setSettings(undefined);
    try {
      const [vaults, configuration] = await Promise.all([
        call('memory.vaults.get', { workspace_id: workspaceId ?? null }),
        call('memory.settings.get', {}),
      ]);
      if (current !== generation.current.value) return;
      setSnapshot(vaults);
      setSettings(configuration);
    } catch (cause) {
      if (current === generation.current.value)
        setError(userMessage(cause, '记忆状态读取失败，请刷新。'));
    }
  }, [call, workspaceId]);
  useEffect(() => {
    const owner = generation.current;
    let disposed = false;
    queueMicrotask(() => {
      if (!disposed) void reload();
    });
    return () => {
      disposed = true;
      owner.value++;
    };
  }, [reload]);
  const act = async <T>(work: () => Promise<T>, success?: string): Promise<T | undefined> => {
    if (locked.current) return undefined;
    locked.current = true;
    setBusy(true);
    setError(undefined);
    setMessage(undefined);
    const current = generation.current.value;
    try {
      const result = await work();
      if (current !== generation.current.value) return undefined;
      if (success) setMessage(success);
      return result;
    } catch (cause) {
      if (current === generation.current.value)
        setError(userMessage(cause, '记忆操作未完成，请刷新查看。'));
      return undefined;
    } finally {
      if (current === generation.current.value) {
        locked.current = false;
        setBusy(false);
      }
    }
  };
  return { snapshot, settings, error, message, busy, call, act, reload };
}
