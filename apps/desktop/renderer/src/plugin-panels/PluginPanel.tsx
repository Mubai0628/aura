import { useEffect, useMemo, useRef, useState } from 'react';

import { useNativeViewOver, type NativeView } from '../native-view/useNativeViewOver';
import { pluginPanelsApi } from './bridge';
import './plugin-panel.css';

interface PluginPanelProps {
  sessionId: string;
  plugin: string;
  visible: boolean;
}

/**
 * A plugin's panel in its tab. The page itself is a view Main lays over the tab's area, in a
 * sandbox of its plugin's own; it is opened (or kept) each time the tab is shown, since Core says
 * each time whether the plugin can still be used. One that stopped says why and stays stopped
 * until the user opens it again.
 */
export function PluginPanel({ sessionId, plugin, visible }: PluginPanelProps) {
  const api = pluginPanelsApi();
  const area = useRef<HTMLDivElement>(null);
  // Each "重新打开" is a new attempt; what Main answered, or why the panel stopped, belongs to one.
  const [attempt, setAttempt] = useState(0);
  const [answer, setAnswer] = useState<{ attempt: number; problem?: string | undefined }>();
  const [stopped, setStopped] = useState<{ attempt: number; why: string }>();
  const halted = stopped?.attempt === attempt;

  useEffect(() => {
    if (!visible || api === undefined || halted) return;
    let live = true;
    void api.command({ kind: 'open', sessionId, plugin }).then((result) => {
      if (live) setAnswer({ attempt, problem: result.ok ? undefined : result.message });
    });
    return () => {
      live = false;
    };
  }, [api, sessionId, plugin, visible, attempt, halted]);

  useEffect(
    () =>
      api?.onState((state) => {
        if (state.sessionId === sessionId && state.plugin === plugin)
          setStopped({ attempt, why: state.stopped });
      }),
    [api, sessionId, plugin, attempt],
  );

  const view = useMemo<NativeView | undefined>(() => {
    if (api === undefined) return undefined;
    return {
      show: (bounds) => api.command({ kind: 'show', sessionId, plugin, bounds }),
      hide: () => void api.command({ kind: 'hide', sessionId, plugin }),
      picture: async () => {
        const result = await api.command({ kind: 'picture', sessionId, plugin });
        return result.ok ? result.picture : undefined;
      },
    };
  }, [api, sessionId, plugin]);
  const current = answer?.attempt === attempt ? answer : undefined;
  const problem = api === undefined ? '插件面板不可用' : halted ? stopped?.why : current?.problem;
  const opened = current !== undefined && problem === undefined;
  const picture = useNativeViewOver(area, view, visible && opened);

  return (
    <div className="plugin-panel">
      <div className="plugin-panel-area" ref={area}>
        {problem !== undefined ? (
          <div className="plugin-panel-stopped">
            <p role="status">{problem}</p>
            {api === undefined ? null : (
              <button
                className="button"
                onClick={() => setAttempt((count) => count + 1)}
                type="button"
              >
                重新打开
              </button>
            )}
          </div>
        ) : picture !== undefined ? (
          <img alt="" className="plugin-panel-picture" draggable={false} src={picture} />
        ) : null}
      </div>
    </div>
  );
}
