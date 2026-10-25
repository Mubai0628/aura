import { useEffect, useRef } from 'react';
import { AppIcon, type IconName } from '../app-icons';
import { ToolIcon } from '../tool-icon';
import { useContextMenu, type MenuEntry } from '../ui/useContextMenu';
import { useScrollingTabRow } from '../ui/useScrollingTabRow';
import type { ToolTab } from './tool-tabs';

export interface StripTab {
  tab: ToolTab;
  label: string;
  icon: IconName;
  title?: string | undefined;
  /** Named after its command (a model's terminal) rather than its tool: it gives way first. */
  givesWay?: boolean;
}

/**
 * With many tabs open, the last ones were hidden and the wheel did not reach
 * them. A tab named after its command gives way, down to a few characters; one named after its
 * tool keeps its name. Past that the row scrolls sideways under the wheel, the tab being shown is
 * kept in view, the edges fade where more tabs are, and 全部标签 lists them all. A tab's
 * right-click menu closes it, the others or those to its right; a 文件 page's files have
 * their own small tabs and menus inside the page.
 */
export function ToolTabStrip({
  tabs,
  active,
  onActivate,
  onClose,
  onCloseMany,
}: {
  tabs: StripTab[];
  active: string | undefined;
  onActivate: (tab: ToolTab) => void;
  onClose: (tab: ToolTab) => void;
  onCloseMany: (tabs: ToolTab[]) => void;
}) {
  const frame = useRef<HTMLDivElement>(null);
  const strip = useRef<HTMLDivElement>(null);
  const menu = useContextMenu();
  // A name that changes (a model's terminal named after its command) can make the row too wide.
  const names = tabs.map((item) => item.label).join('\n');
  const overflow = useScrollingTabRow(strip, frame, names);

  useEffect(() => {
    strip.current
      ?.querySelector('.tool-tab.is-active')
      ?.scrollIntoView?.({ block: 'nearest', inline: 'nearest' });
  }, [active, tabs.length]);

  const entriesFor = (item: StripTab): MenuEntry[] => {
    const index = tabs.indexOf(item);
    const others = tabs.filter((other) => other !== item).map((other) => other.tab);
    const right = tabs.slice(index + 1).map((other) => other.tab);
    return [
      { label: '关闭', icon: <AppIcon name="close" size={14} />, run: () => onClose(item.tab) },
      { label: '关闭其他标签', disabled: !others.length, run: () => onCloseMany(others) },
      { label: '关闭右侧标签', disabled: !right.length, run: () => onCloseMany(right) },
      { label: '关闭全部标签', run: () => onCloseMany(tabs.map((each) => each.tab)) },
    ];
  };

  return (
    <div
      className="right-tab-strip-frame"
      data-overflow-left={overflow.left || undefined}
      data-overflow-right={overflow.right || undefined}
      ref={frame}
    >
      <div className="right-tab-strip" role="tablist" aria-label="已打开的工具" ref={strip}>
        {tabs.length === 0 ? <span className="right-workspace-label">工具工作区</span> : null}
        {tabs.map((item) => (
          <div
            className={`tool-tab${item.tab.id === active ? ' is-active' : ''}${item.givesWay ? ' gives-way' : ''}`}
            key={item.tab.id}
            onContextMenu={(event) => menu.open(event, `${item.label} 标签`, entriesFor(item))}
          >
            <button
              aria-selected={item.tab.id === active}
              onClick={() => onActivate(item.tab)}
              role="tab"
              title={item.title ? `${item.label}\n${item.title}` : item.label}
              type="button"
            >
              <AppIcon name={item.icon} size={14} />
              <span>{item.label}</span>
            </button>
            <button
              aria-label={`关闭${item.label}`}
              className="icon-button"
              onClick={() => onClose(item.tab)}
              type="button"
            >
              <AppIcon name="close" size={13} />
            </button>
          </div>
        ))}
      </div>
      {overflow.crowded ? (
        <button
          aria-haspopup="menu"
          aria-label="全部标签"
          className="icon-button right-tab-list-button"
          onClick={(event) => {
            const bounds = event.currentTarget.getBoundingClientRect();
            menu.openAt(
              bounds.left,
              bounds.bottom + 4,
              '全部标签',
              tabs.map((item) => ({
                label: item.label,
                icon:
                  item.tab.id === active ? (
                    <ToolIcon name="check" size={14} />
                  ) : (
                    <AppIcon name={item.icon} size={14} />
                  ),
                run: () => onActivate(item.tab),
              })),
            );
          }}
          title="全部标签"
          type="button"
        >
          <AppIcon name="chevron-down" size={14} />
        </button>
      ) : null}
      {menu.menu}
    </div>
  );
}
