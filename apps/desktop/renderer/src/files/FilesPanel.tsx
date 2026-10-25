import { useEffect, useId, useRef, useState, type CSSProperties } from 'react';
import { AppIcon } from '../app-icons';
import { CollapsiblePaneResizer } from '../CollapsiblePaneResizer';
import { toolTabs, type ToolTab } from '../right-workspace/tool-tabs';
import { useContextMenu } from '../ui/useContextMenu';
import { FileTree } from './FileTree';
import { FileView } from './FileView';
import { carriedTabEntries } from './carried-menu';
import { fileMenuEntries, type FileActionScope } from './file-menu';
import { entryName, inWorkspace } from './file-sources';
import { fileTrees, useFileTree } from './file-tree-store';
import {
  FILES_TREE_MIN_WIDTH,
  filesTreeMaxWidth,
  readFilesTreeWidth,
  writeFilesTreeWidth,
} from './files-tree-width';
import './files.css';

/** The page's own width, as it changes with the panel. */
function usePageWidth() {
  const page = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useEffect(() => {
    const element = page.current;
    if (!element || typeof ResizeObserver === 'undefined') return;
    const observer = new ResizeObserver(() => setWidth(element.clientWidth));
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  return { page, width };
}

/**
 * A 文件 tab, read only, after the owner's reference: the workspace tree
 * beside the open file, shown or hidden with one button. "打开的文件在文件页
 * 内的文件树右侧半栏展示，控制关闭文件查看标签页" — the files opened here are the page's own small
 * tabs beside the tree, each closed on its own; with none open the tree has the whole page. The
 * tree marks the file shown and opens its folders.
 */
export function FilesPanel({
  scope,
  tab,
  visible,
  refreshKey,
}: {
  scope: FileActionScope;
  tab: ToolTab;
  visible: boolean;
  refreshKey?: string | undefined;
}) {
  const tree = useFileTree(scope);
  const menu = useContextMenu();
  const files = tab.files ?? [];
  const shown = tab.file;
  const treeId = useId();
  const { page, width: pageWidth } = usePageWidth();
  const [treeWidth, setTreeWidth] = useState(readFilesTreeWidth);
  const maximum = filesTreeMaxWidth(pageWidth);
  const shownWidth = Math.min(treeWidth, maximum);
  useEffect(() => {
    const folder = shown?.split('/').slice(0, -1).join('/');
    if (folder) fileTrees.reveal(scope, folder);
  }, [scope, shown]);
  const treeButton = (
    <button
      aria-label={tree.open ? '隐藏文件树' : '显示文件树'}
      aria-pressed={tree.open}
      className="icon-button"
      onClick={() => fileTrees.setOpen(scope, !tree.open)}
      title={tree.open ? '隐藏文件树' : '显示文件树'}
      type="button"
    >
      <AppIcon name="sidebar-left" size={15} />
    </button>
  );
  return (
    <section aria-label={shown ?? '文件'} className="files-panel" hidden={!visible}>
      <div
        className="files-layout"
        data-tree={tree.open ? 'open' : 'closed'}
        data-view={shown ? 'file' : 'none'}
        ref={page}
        style={{ '--files-tree-width': `${shownWidth}px` } as CSSProperties}
      >
        {tree.open ? (
          <FileTree
            id={treeId}
            scope={scope}
            current={shown && inWorkspace(shown) ? shown : undefined}
            onPick={(path) => toolTabs.openFile(scope.session, path)}
            onMenu={(event, path) =>
              menu.open(event, `${path} 的操作`, fileMenuEntries(scope, path, 'tree'))
            }
          />
        ) : null}
        {tree.open && shown ? (
          <CollapsiblePaneResizer
            className="files-tree-resizer"
            controls={treeId}
            direction="left"
            label="调整文件树宽度"
            maximum={maximum}
            minimum={FILES_TREE_MIN_WIDTH}
            onCollapse={() => fileTrees.setOpen(scope, false)}
            onResize={setTreeWidth}
            onResizeCommit={(width) => {
              setTreeWidth(width);
              writeFilesTreeWidth(width);
            }}
            width={shownWidth}
          />
        ) : null}
        {shown ? (
          <div className="files-viewer">
            <div className="files-open-row">
              {treeButton}
              <div aria-label="打开的文件" className="files-open-tabs" role="tablist">
                {files.map((path) => (
                  <div
                    className="files-open-tab"
                    data-shown={path === shown || undefined}
                    key={path}
                    onContextMenu={(event) =>
                      menu.open(
                        event,
                        `${entryName(path)} 的操作`,
                        inWorkspace(path)
                          ? fileMenuEntries(scope, path, 'tab')
                          : carriedTabEntries(scope, path),
                      )
                    }
                  >
                    <button
                      aria-selected={path === shown}
                      onClick={() => toolTabs.showFile(scope.session, tab.id, path)}
                      role="tab"
                      title={inWorkspace(path) ? path : entryName(path)}
                      type="button"
                    >
                      {entryName(path)}
                    </button>
                    <button
                      aria-label={`关闭 ${entryName(path)}`}
                      className="icon-button"
                      onClick={() => toolTabs.closeFile(scope.session, tab.id, path)}
                      title={`关闭 ${entryName(path)}`}
                      type="button"
                    >
                      <AppIcon name="close" size={12} />
                    </button>
                  </div>
                ))}
              </div>
            </div>
            <FileView
              scope={scope}
              path={shown}
              line={tab.line}
              shown={tab.shown}
              visible={visible}
              refreshKey={refreshKey}
            />
          </div>
        ) : tree.open ? null : (
          <div className="file-view">
            <header className="file-view-header">
              {treeButton}
              <span className="file-view-path">文件</span>
            </header>
            <div className="files-empty">
              <AppIcon name="file" size={28} />
              <h3>打开的文件会显示在这里</h3>
              <p>在文件树里选一个文件，或点对话里的文件路径。</p>
            </div>
          </div>
        )}
      </div>
      {menu.menu}
    </section>
  );
}
