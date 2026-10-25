import { useEffect, useState } from 'react';

import { useActivitySession } from './activity-session';

type Loaded = { kind: 'image'; url: string } | { kind: 'missing'; text: string };

/**
 * The screenshot a browser call took, under the call in the conversation. It is read from
 * the conversation's own record when the row opens; the model may or may not have seen it (the
 * row's line says which).
 */
export function BrowserScreenshot({ artifactId }: { artifactId: string }) {
  const sessionId = useActivitySession();
  const [loaded, setLoaded] = useState<Loaded>();
  useEffect(() => {
    if (sessionId === undefined) return;
    let current = true;
    void window.aura.v1.browser
      ?.readScreenshot({ session_id: sessionId, artifact_id: artifactId })
      .then((result) => {
        if (!current) return;
        setLoaded(
          result.ok
            ? {
                kind: 'image',
                url: `data:${result.value.media_type};base64,${result.value.data_base64}`,
              }
            : { kind: 'missing', text: '截图不可用' },
        );
      });
    return () => {
      current = false;
    };
  }, [sessionId, artifactId]);
  if (loaded === undefined) return null;
  if (loaded.kind === 'missing')
    return (
      <p className="agent-tool-screenshot-missing" role="status">
        {loaded.text}
      </p>
    );
  return (
    <figure className="agent-tool-screenshot">
      <img alt="网页截图" src={loaded.url} />
    </figure>
  );
}
