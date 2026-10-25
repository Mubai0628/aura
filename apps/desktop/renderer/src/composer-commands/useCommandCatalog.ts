import type { CommandEntry } from '@aura/contracts/generated';
import { useCallback, useEffect, useState } from 'react';

const FAILED = '命令列表读取失败，请稍后再试。';

/** The Core command catalog; `reload` is called whenever the menu opens so file edits show up. */
export function useCommandCatalog(workspaceId: string | undefined) {
  const [generation, setGeneration] = useState(0);
  const [loaded, setLoaded] = useState<{ commands: CommandEntry[]; error?: string }>({
    commands: [],
  });
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
          result.ok ? { commands: result.value.commands } : { ...current, error: FAILED },
        );
      })
      .catch(() => {
        if (live) setLoaded((current) => ({ ...current, error: FAILED }));
      });
    return () => {
      live = false;
    };
  }, [workspaceId, generation]);
  const reload = useCallback(() => setGeneration((value) => value + 1), []);
  return { commands: loaded.commands, error: loaded.error, reload };
}
