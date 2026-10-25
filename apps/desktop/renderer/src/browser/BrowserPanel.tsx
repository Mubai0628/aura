import { useState } from 'react';

import { BrowserHome } from './BrowserHome';
import { BrowserPage } from './BrowserPage';
import { BrowserTabStrip } from './BrowserTabStrip';
import { browserTabs, openInBrowser, useBrowserTabs } from './browser-tabs';
import './browser-panel.css';

/**
 * Aura's built-in browser in the right sidebar. One 浏览器 tab whose
 * pages are tabs of its own, a start page when none is open, and a button that lets it fill the
 * main area.
 */
export function BrowserPanel({
  sessionId,
  visible,
  expanded = false,
  onToggleExpanded,
}: {
  sessionId: string;
  visible: boolean;
  expanded?: boolean;
  onToggleExpanded?: (() => void) | undefined;
}) {
  const { pages, shown } = useBrowserTabs(sessionId);
  // The page the user opened with "+" takes the keyboard; one the model opened does not.
  const [opened, setOpened] = useState<string>();
  return (
    <section aria-label="浏览器" className="browser-panel">
      <BrowserTabStrip
        expanded={expanded}
        onNewPage={() => setOpened(browserTabs.open(sessionId))}
        onToggleExpanded={onToggleExpanded}
        pages={pages}
        sessionId={sessionId}
        shown={shown}
      />
      {shown === undefined ? (
        <BrowserHome onOpen={(input) => openInBrowser(sessionId, { address: input })} />
      ) : (
        <BrowserPage
          focus={shown === opened}
          key={shown}
          pageId={shown}
          sessionId={sessionId}
          visible={visible}
        />
      )}
    </section>
  );
}
