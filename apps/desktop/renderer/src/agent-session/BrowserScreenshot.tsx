import { useCallback, useContext, useEffect, useState } from 'react';

import { PicturePreview } from '../composer-attachments/PicturePreview';
import { pictureMenuEntries } from '../composer-attachments/picture-menu';
import { readPicture } from '../composer-attachments/picture-reads';
import { FileLinkContext } from '../files/file-links';
import { useContextMenu } from '../ui/useContextMenu';
import { useActivitySession } from './activity-session';

type Loaded = { kind: 'image'; url: string } | { kind: 'missing'; text: string };

/**
 * The screenshot a browser, Chrome or desktop call took, under the call in the conversation. It
 * is read from the conversation's own record when the row opens, and kept, so the row opens again
 * at once; the model may or may not have seen it (the row's line says which). A click opens it in
 * the picture viewer; its menu also copies or saves it.
 */
export function BrowserScreenshot({ artifactId, name }: { artifactId: string; name: string }) {
  const sessionId = useActivitySession();
  const scope = useContext(FileLinkContext);
  const [loaded, setLoaded] = useState<Loaded>();
  const [previewing, setPreviewing] = useState(false);
  const menu = useContextMenu();
  // Stable while the viewer is open, so its focus is kept for its whole life.
  const close = useCallback(() => setPreviewing(false), []);
  useEffect(() => {
    if (sessionId === undefined) return;
    let current = true;
    void readPicture(sessionId, artifactId).then((url) => {
      if (!current || url === undefined) return;
      setLoaded(url === null ? { kind: 'missing', text: '截图不可用' } : { kind: 'image', url });
    });
    return () => {
      current = false;
    };
  }, [sessionId, artifactId]);
  if (loaded === undefined || sessionId === undefined) return null;
  if (loaded.kind === 'missing')
    return (
      <p className="agent-tool-screenshot-missing" role="status">
        {loaded.text}
      </p>
    );
  return (
    <figure className="agent-tool-screenshot">
      <button
        aria-label={`预览 ${name}`}
        className="agent-tool-screenshot-open"
        onClick={() => setPreviewing(true)}
        onContextMenu={(event) =>
          menu.open(
            event,
            `${name} 的操作`,
            pictureMenuEntries(scope, sessionId, artifactId, name, () => setPreviewing(true)),
          )
        }
        type="button"
      >
        <img alt={name} src={loaded.url} />
      </button>
      {previewing ? <PicturePreview name={name} onClose={close} src={loaded.url} /> : null}
      {menu.menu}
    </figure>
  );
}
