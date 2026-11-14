import { useCallback, useEffect, useRef, useState } from 'react';
import type { PackageListV1 } from '@aura/contracts/generated';
import type {
  PackageMainCommand,
  PackageMainParams,
  PackageMainResult,
} from '../../../shared/package-api';
import { packageFailure, packageNotSent } from './package-words';

export type PackageCall = <C extends PackageMainCommand>(
  command: C,
  params: PackageMainParams[C],
) => Promise<PackageMainResult[C] | undefined>;

/** One package page state: the list, one busy flag and one message line. */
export function usePackages() {
  const [list, setList] = useState<PackageListV1>();
  const [message, setMessage] = useState<string>();
  // Why the list was never read; the page offers to read it again.
  const [listError, setListError] = useState<string>();
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
      setMessage(packageFailure(command, params, result.error));
    } catch {
      if (live.current) setMessage(packageNotSent(command, params));
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
  /** Reads the list for the page, and why when it was not read. */
  const load = useCallback(
    () =>
      window.aura.v1.packages.invoke('packages.list', {}).then(
        (result) => {
          if (!live.current) return;
          if (result.ok) {
            setList(result.value);
            setListError(undefined);
          } else setListError(packageFailure('packages.list', {}, result.error));
        },
        () => {
          if (live.current) setListError(packageNotSent('packages.list', {}));
        },
      ),
    [],
  );
  useEffect(() => {
    void load();
  }, [load]);
  return { list, listError, load, message, setMessage, busy, call, act, refresh };
}
