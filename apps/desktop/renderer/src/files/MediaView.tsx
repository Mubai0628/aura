import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type CSSProperties,
} from 'react';
import { AppIcon } from '../app-icons';
import { userMessage } from '../agent-session/user-message';
import { PicturePreview } from '../composer-attachments/PicturePreview';
import { fileSource, outsideTarget } from './file-sources';
import type { FileScope } from './file-tree-store';

/**
 * A picture fitted to the tab or at its own
 * size, a video or a sound with its player. What the player cannot play opens with its program.
 * A click on the
 * picture opens the same full-window viewer as the composer's pictures.
 */
export function MediaView({
  scope,
  path,
  name,
  media,
  src,
  actual,
  visible = true,
}: {
  scope: FileScope;
  path: string;
  name: string;
  media: 'image' | 'video' | 'audio';
  src: string;
  actual: boolean;
  visible?: boolean;
}) {
  const [failed, setFailed] = useState(false);
  const [refused, setRefused] = useState<string>();
  const [expanded, setExpanded] = useState(false);
  const [previewing, setPreviewing] = useState(false);
  const [bounds, setBounds] = useState<CSSProperties>();
  const host = useRef<HTMLDivElement>(null);
  if (!visible && expanded) setExpanded(false);
  // Stable while the viewer is open, so its focus is kept for its whole life.
  const closePreview = useCallback(() => setPreviewing(false), []);
  useEffect(() => {
    if (!expanded) return;
    const keyDown = (event: KeyboardEvent) => {
      if (event.defaultPrevented || document.querySelector('dialog.picture-preview[open]')) return;
      if (event.key === 'Escape') {
        event.preventDefault();
        setExpanded(false);
      }
    };
    window.addEventListener('keydown', keyDown);
    return () => window.removeEventListener('keydown', keyDown);
  }, [expanded]);
  useLayoutEffect(() => {
    const element = host.current;
    if (!expanded || !element) return;
    const sidebar = element.closest<HTMLElement>('.right-workspace');
    const toolbar = sidebar?.querySelector('.right-workspace-toolbar');
    const measure = () => {
      if (!sidebar) return;
      const rect = sidebar.getBoundingClientRect();
      const top = toolbar?.getBoundingClientRect().bottom ?? rect.top;
      setBounds({
        left: rect.left,
        top,
        width: rect.width,
        height: Math.max(0, rect.bottom - top),
      });
    };
    measure();
    // The top layer escapes file-tree containment without replacing the playing video.
    element.showPopover?.();
    const observer = new ResizeObserver(measure);
    if (sidebar) observer.observe(sidebar);
    if (toolbar) observer.observe(toolbar);
    window.addEventListener('resize', measure);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', measure);
      if (element.isConnected && element.hidePopover && element.matches(':popover-open'))
        element.hidePopover();
    };
  }, [expanded]);
  const openOutside = async () => {
    const outside = outsideTarget(fileSource(path), scope.session);
    if (outside) {
      const opened = await window.aura.v1.attachmentFile({
        ...outside,
        action: 'default_application',
      });
      if (!opened.ok) setRefused(opened.message);
      return;
    }
    try {
      const result = await window.aura.v1.openWorkspaceFileOutside({
        session_id: scope.session,
        workspace_id: scope.workspace,
        path,
        opener: 'default_application',
      });
      if (!result.ok) setRefused(userMessage(result.error, '没有打开这个文件，请重试。'));
      else if (!result.value.completed) setRefused(result.value.message);
    } catch (error) {
      setRefused(userMessage(error, '没有打开这个文件，请重试。'));
    }
  };
  if (failed)
    return (
      <div className="file-view-status" role="alert">
        <p>播放不了这个文件。</p>
        <button onClick={() => void openOutside()} type="button">
          用系统程序打开
        </button>
        {refused ? <p>{refused}</p> : null}
      </div>
    );
  if (media === 'image')
    return (
      <div className="file-view-image" data-actual={actual || undefined}>
        <button
          aria-label={`预览 ${name}`}
          className="file-view-image-open"
          onClick={() => setPreviewing(true)}
          type="button"
        >
          <img alt={name} onError={() => setFailed(true)} src={src} />
        </button>
        {previewing ? <PicturePreview name={name} onClose={closePreview} src={src} /> : null}
      </div>
    );
  return (
    <div
      className="file-view-media"
      data-expanded={expanded || undefined}
      popover={expanded ? 'manual' : undefined}
      ref={host}
      style={expanded ? bounds : undefined}
    >
      {media === 'video' ? (
        <>
          <video
            controls
            controlsList="nofullscreen"
            onError={() => setFailed(true)}
            preload="metadata"
            src={src}
          />
          <button
            aria-label={expanded ? '还原视频大小' : '填满侧边栏'}
            aria-pressed={expanded}
            className="icon-button media-expand"
            onClick={() => setExpanded((value) => !value)}
            title={expanded ? '还原视频大小' : '填满侧边栏'}
            type="button"
          >
            <AppIcon name={expanded ? 'shrink' : 'expand'} size={16} />
          </button>
        </>
      ) : (
        <audio controls onError={() => setFailed(true)} preload="metadata" src={src} />
      )}
    </div>
  );
}
