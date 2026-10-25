import { userMessage } from './agent-session/user-message';
import { openInBrowser } from './browser/browser-tabs';
import { copyText } from './clipboard';
import type { FileLinkScope } from './files/file-links';
import { ToolIcon } from './tool-icon';
import type { MenuEntry } from './ui/ContextMenu';

/** An address the conversation may open in the browser: http or https, nothing else. */
export function webAddress(url: string): string | undefined {
  try {
    const parsed = new URL(url);
    return parsed.protocol === 'http:' || parsed.protocol === 'https:' ? url : undefined;
  } catch {
    return undefined;
  }
}

/**
 * Main opens the page in the default browser after
 * its own check; a refusal or a failure is said.
 */
export async function openWebLink(scope: FileLinkScope, url: string): Promise<void> {
  try {
    const result = await window.aura.v1.openExternalLink({ url });
    if (!result.ok) scope.notify(userMessage(result.error, '没有打开这个链接，请重试。'));
    else if (!result.value.completed) scope.notify(result.value.message);
  } catch (error) {
    scope.notify(userMessage(error, '没有打开这个链接，请重试。'));
  }
}

/**
 * A link's right-click menu: open it in the default browser or in Aura's own; or copy its address.
 */
export function webLinkEntries(scope: FileLinkScope, url: string): MenuEntry[] {
  return [
    {
      label: '在默认浏览器中打开',
      icon: <ToolIcon name="external-link" size={14} />,
      run: () => void openWebLink(scope, url),
    },
    {
      label: '在内置浏览器打开',
      icon: <ToolIcon name="globe" size={14} />,
      run: () =>
        void openInBrowser(scope.session, { address: url }).then(
          (refused) => refused && scope.notify(refused),
        ),
    },
    {
      label: '复制链接',
      icon: <ToolIcon name="copy" size={14} />,
      run: () => {
        void copyText(url, '没能复制链接，请重试。').then(
          (refused) => refused && scope.notify(refused),
        );
      },
    },
  ];
}
