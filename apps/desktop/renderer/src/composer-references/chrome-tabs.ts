import { useEffect, useState } from 'react';
import { chromeControl } from '../chrome/bridge';
import type { CaptureOption } from './captures';

/** The user's Chrome tabs, and why the connected extension could not list them. */
export interface ChromeTabs {
  options: readonly CaptureOption[];
  problem?: string | undefined;
}

const NONE: ChromeTabs = { options: [] };

/**
 * Read once per opening of the "@" list while a conversation is open: a tab picked is shared with
 * it when the message is sent. Without Chrome connected there are none.
 */
export function useChromeTabs(open: boolean, session: string | undefined): ChromeTabs {
  const [listed, setListed] = useState<ChromeTabs>(NONE);
  useEffect(() => {
    if (!open || !session) return;
    let live = true;
    // A desktop without this call still lists everything else.
    void Promise.resolve()
      .then(() => chromeControl()?.tabs())
      .then(
        (result) => {
          if (!live || !result?.ok) return;
          setListed({
            options: result.value.tabs.map((tab) => ({
              kind: 'chrome',
              id: String(tab.tab_id),
              title: tab.title.trim() || tab.url,
              detail: tab.url,
            })),
            problem: result.value.issue ?? undefined,
          });
        },
        () => undefined,
      );
    return () => {
      live = false;
      setListed(NONE);
    };
  }, [open, session]);
  return listed;
}
