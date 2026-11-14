import type { CommandEntry } from '@aura/contracts/generated';
import { useCallback, useEffect, useState } from 'react';
import { failedAction } from '../agent-session/failure-text';

/** A read that failed: with no list yet nothing can be chosen; else the last list stays. */
const failed = (error: unknown, kept: boolean): string =>
  kept
    ? failedAction(
        error,
        '命令列表没能重新读取',
        '显示的是上一次的列表；关闭菜单后再输入 / 会重新读取。',
      )
    : failedAction(error, '命令列表读不出来', '这次无法选择命令。关闭菜单后再输入 / 会重新读取。');

type Loaded = { commands: CommandEntry[]; error?: string };

const failedFrom = (current: Loaded, error: unknown): Loaded => ({
  ...current,
  error: failed(error, current.commands.length > 0),
});

/** The Core command catalog; `reload` is called whenever the menu opens so file edits show up. */
export function useCommandCatalog(workspaceId: string | undefined) {
  const [generation, setGeneration] = useState(0);
  const [loaded, setLoaded] = useState<Loaded>({ commands: [] });
  useEffect(() => {
    const api = window.aura?.v1;
    if (!api?.listCommands) return;
    let live = true;
    api
      .listCommands({ workspace_id: workspaceId ?? null })
      .then((result) => {
        if (!live) return;
        // A failed reread keeps the last list, so the menu still works while it says so.
        setLoaded((current) =>
          result.ok ? { commands: result.value.commands } : failedFrom(current, result.error),
        );
      })
      .catch((reason: unknown) => {
        if (live) setLoaded((current) => failedFrom(current, reason));
      });
    return () => {
      live = false;
    };
  }, [workspaceId, generation]);
  const reload = useCallback(() => setGeneration((value) => value + 1), []);
  return { commands: loaded.commands, error: loaded.error, reload };
}
