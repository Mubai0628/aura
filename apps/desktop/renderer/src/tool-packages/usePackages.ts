import { userMessage } from '../agent-session/user-message';
import { useCallback, useEffect, useRef, useState } from 'react';
import type { PackageListV1 } from '@aura/contracts/generated';
import type {
  PackageMainCommand,
  PackageMainParams,
  PackageMainResult,
} from '../../../shared/package-api';

export type PackageCall = <C extends PackageMainCommand>(
  command: C,
  params: PackageMainParams[C],
) => Promise<PackageMainResult[C] | undefined>;

/** One package page state: the list, one busy flag and one message line. */
export function usePackages() {
  const [list, setList] = useState<PackageListV1>();
  const [message, setMessage] = useState<string>();
  const [busy, setBusy] = useState(false);
  const live = useRef(true);
  const working = useRef(false);
  useEffect(() => {
    live.current = true;
    return () => {
      live.current = false;
    };
  }, []);
  const call: PackageCall = useCallback(async (command, params) => {
    try {
      const result = await window.aura.v1.packages.invoke(command, params);
      if (!live.current) return undefined;
      if (result.ok) return result.value;
      setMessage(userMessage(result.error, '操作没有完成，请刷新后核对。'));
    } catch {
      if (live.current) setMessage('工具包请求未完成，请刷新后查看结果。');
    }
    return undefined;
  }, []);
  const refresh = useCallback(async () => {
    const value = await call('packages.list', {});
    if (value !== undefined && live.current) setList(value);
  }, [call]);
  /** Runs one user action at a time and refreshes the list after it. */
  const act = useCallback(
    async <T>(action: () => Promise<T | undefined>): Promise<T | undefined> => {
      if (working.current) return undefined;
      working.current = true;
      setBusy(true);
      setMessage(undefined);
      try {
        const value = await action();
        await refresh();
        return value;
      } finally {
        working.current = false;
        if (live.current) setBusy(false);
      }
    },
    [refresh],
  );
  useEffect(() => {
    let current = true;
    void window.aura.v1.packages
      .invoke('packages.list', {})
      .then((result) => {
        if (!current) return;
        if (result.ok) setList(result.value);
        else setMessage(userMessage(result.error, '工具包列表读取失败，请重试。'));
      })
      .catch(() => {
        if (current) setMessage('无法读取工具包列表，请重试。');
      });
    return () => {
      current = false;
    };
  }, []);
  return { list, message, setMessage, busy, call, act, refresh };
}
