import type { CommandEntry } from '@aura/contracts/generated';
import { useEffect, useRef } from 'react';
import './composer-commands.css';

export interface CommandOption {
  command: CommandEntry;
  /** Why it cannot run now; such options are shown greyed and cannot be picked. */
  reason?: string | undefined;
}

const SOURCE_LABELS = {
  builtin: '',
  user: '我的命令',
  workspace: '工作区命令',
  plugin: '插件',
} as const;

/** Where a command comes from, naming the plugin that brings it. */
const sourceLabel = (command: CommandEntry): string =>
  command.plugin ? `插件 ${command.plugin}` : SOURCE_LABELS[command.source];

/**
 * The list behind `/` and `+ → 命令`. Focus stays in the composer while it is open; a press
 * anywhere outside the list closes it, and the line ↑↓ chose stays in view.
 */
export function CommandMenu({
  options,
  highlight,
  error,
  onPick,
  onHover,
  onDismiss,
}: {
  options: readonly CommandOption[];
  highlight: number;
  error?: string | undefined;
  onPick: (option: CommandOption) => void;
  onHover: (index: number) => void;
  onDismiss: () => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const selected = root.current?.querySelector<HTMLElement>('[aria-selected="true"]');
    // jsdom has no scrollIntoView; the desktop always does.
    if (selected && 'scrollIntoView' in selected) selected.scrollIntoView({ block: 'nearest' });
  }, [highlight, options]);
  useEffect(() => {
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) onDismiss();
    };
    document.addEventListener('pointerdown', outside, true);
    return () => document.removeEventListener('pointerdown', outside, true);
  }, [onDismiss]);
  return (
    <div ref={root} className="composer-command-menu composer-popover">
      <p>命令{error ? ` · ${error}` : ''}</p>
      {options.length === 0 ? (
        <p role="status">没有匹配的命令；继续输入会作为普通文字发送。</p>
      ) : (
        <ul role="listbox" aria-label="命令">
          {options.map((option, index) => (
            <li
              role="option"
              key={`${option.command.source}:${option.command.name}`}
              aria-selected={index === highlight}
              aria-disabled={option.reason !== undefined}
              // Keep focus (and the caret) in the composer.
              onMouseDown={(event) => {
                event.preventDefault();
                onPick(option);
              }}
              onMouseEnter={() => onHover(index)}
            >
              <span className="command-name">/{option.command.name}</span>
              <span className="command-title">{option.command.title}</span>
              {sourceLabel(option.command) ? (
                <span className="command-source">{sourceLabel(option.command)}</span>
              ) : null}
              <small>{option.reason ?? option.command.description}</small>
            </li>
          ))}
        </ul>
      )}
      <p className="command-menu-hint">↑↓ 选择 · Enter/Tab 确认 · Esc 关闭（已输入的文字保留）</p>
    </div>
  );
}
