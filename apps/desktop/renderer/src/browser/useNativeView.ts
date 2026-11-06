import { useMemo, type RefObject } from 'react';

import { useNativeViewOver, type NativeView } from '../native-view/useNativeViewOver';
import { browserApi } from './bridge';

/**
 * A page of Aura's built-in browser: a native view Main lays over the page area. While `showing`,
 * it is kept on that area, out of the way of dialogs and menus drawn over it; otherwise it is
 * hidden. Returns a picture of the page to draw in the area while a dialog or menu covers it.
 */
export function useNativeView(
  area: RefObject<HTMLDivElement | null>,
  sessionId: string,
  pageId: string,
  showing: boolean,
): string | undefined {
  const view = useMemo<NativeView | undefined>(() => {
    const api = browserApi();
    if (api === undefined) return undefined;
    return {
      show: (bounds) => api.command({ kind: 'show', sessionId, pageId, bounds }),
      hide: () => void api.command({ kind: 'hide', sessionId, pageId }),
      picture: async () => {
        const result = await api.command({ kind: 'picture', sessionId, pageId });
        return result.ok ? result.picture : undefined;
      },
    };
  }, [sessionId, pageId]);
  return useNativeViewOver(area, view, showing);
}
