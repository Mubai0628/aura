import { useEffect, useRef } from 'react';
import type { McpResourceProblem } from '@aura/contracts/generated';
import { AppIcon, type IconName } from '../app-icons';
import { ProgramIcon } from './ProgramIcon';
import type { ReferenceOption } from './useReferenceMenu';
import './composer-references.css';

const ICONS: Record<ReferenceOption['kind'], IconName> = {
  file: 'file',
  folder: 'folder',
  skill: 'book',
  session: 'chat',
  terminal: 'terminal',
  page: 'browser',
  resource: 'plug',
  program: 'desktop',
};

/** The groups after the workspace's files and folders, in the order the list shows them. */
const GROUPS: readonly [ReferenceOption['kind'], string][] = [
  ['skill', 'Skill'],
  ['session', '会话'],
  ['terminal', '终端'],
  ['page', '浏览器标签页'],
  ['resource', 'MCP 资源'],
  ['program', '电脑上的程序'],
];

/**
 * The list behind a typed "@": the workspace's files and folders, then the usable Skills, the
 * other conversations, this conversation's terminals and browser pages, the resources of its ready
 * MCP servers with the servers that could not list theirs, and the computer's programs. Focus
 * stays in the composer
 * while it is open; a press anywhere outside the list closes it, and the line ↑↓ chose stays in
 * view.
 */
export function ReferenceMenu({
  options,
  highlight,
  searching,
  available,
  error,
  resourceProblems = [],
  onPick,
  onHover,
  onDismiss,
}: {
  options: readonly ReferenceOption[];
  highlight: number;
  searching: boolean;
  available: boolean;
  error?: string | undefined;
  resourceProblems?: readonly McpResourceProblem[] | undefined;
  onPick: (option: ReferenceOption) => void;
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
  const paths = options.filter((option) => option.kind === 'file' || option.kind === 'folder');
  const status = !available
    ? '没有可查找的工作区'
    : error
      ? error
      : searching
        ? '正在查找…'
        : options.length === 0
          ? '没有相符的文件或文件夹'
          : undefined;
  const line = (option: ReferenceOption) => {
    const index = options.indexOf(option);
    return (
      <li
        role="option"
        key={option.key}
        aria-selected={index === highlight}
        aria-disabled={option.loaded ? true : undefined}
        // Keep focus (and the caret) in the composer.
        onMouseDown={(event) => {
          event.preventDefault();
          onPick(option);
        }}
        onMouseEnter={() => onHover(index)}
        title={option.detail ? `${option.label}\n${option.detail}` : option.label}
      >
        {option.kind === 'program' ? (
          <ProgramIcon appId={option.appId} size={16} />
        ) : (
          <AppIcon name={ICONS[option.kind]} size={13} />
        )}
        <span>{option.label}</span>
        {(option.kind === 'page' || option.kind === 'resource') && option.detail ? (
          <small>{option.detail}</small>
        ) : null}
        {option.loaded ? <small>已加载</small> : null}
      </li>
    );
  };
  return (
    <div ref={root} className="composer-reference-menu composer-popover">
      {status || paths.length ? <p>文件和文件夹</p> : null}
      {status ? (
        <p role={error ? 'alert' : 'status'}>{status}</p>
      ) : paths.length ? (
        <ul role="listbox" aria-label="文件和文件夹">
          {paths.map(line)}
        </ul>
      ) : null}
      {GROUPS.map(([kind, name]) => {
        const shown = options.filter((option) => option.kind === kind);
        const problems = kind === 'resource' ? resourceProblems : [];
        return shown.length || problems.length ? (
          <section key={kind}>
            <p>{name}</p>
            {shown.length ? (
              <ul role="listbox" aria-label={name}>
                {shown.map(line)}
              </ul>
            ) : null}
            {problems.map((problem) => (
              <p role="status" key={problem.server_id}>
                {`${problem.server_name}：${problem.reason}`}
              </p>
            ))}
          </section>
        ) : null;
      })}
    </div>
  );
}
