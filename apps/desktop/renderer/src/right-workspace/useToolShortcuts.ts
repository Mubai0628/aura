import { useEffect } from 'react';
import type { ToolKind } from './tool-tabs';

/** The shortcut each tool shows in the tool menu. */
export const TOOL_SHORTCUTS: Partial<Record<ToolKind, string>> = {
  review: 'Ctrl+Shift+G',
  terminal: 'Ctrl+`',
  browser: 'Ctrl+T',
  files: 'Ctrl+P',
  chat: 'Ctrl+Alt+S',
};

function toolFor(event: KeyboardEvent): ToolKind | undefined {
  const key = event.key.toLowerCase();
  if (event.shiftKey && !event.altKey) return key === 'g' ? 'review' : undefined;
  if (event.altKey && !event.shiftKey) return key === 's' ? 'chat' : undefined;
  if (event.altKey || event.shiftKey) return undefined;
  if (event.code === 'Backquote' || key === '`') return 'terminal';
  return key === 'p' ? 'files' : key === 't' ? 'browser' : undefined;
}

/** Opens (or shows) a tool from its shortcut; a modal dialog keeps the keyboard to itself. */
export function useToolShortcuts(show: (kind: ToolKind) => void) {
  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      if (
        event.defaultPrevented ||
        event.isComposing ||
        event.repeat ||
        !event.ctrlKey ||
        event.metaKey
      )
        return;
      const kind = toolFor(event);
      if (kind === undefined || document.querySelector('dialog[open]')) return;
      event.preventDefault();
      show(kind);
    };
    window.addEventListener('keydown', keydown);
    return () => window.removeEventListener('keydown', keydown);
  }, [show]);
}
