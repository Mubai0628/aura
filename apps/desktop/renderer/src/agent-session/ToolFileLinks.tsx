import { useContext } from 'react';
import { openPlaced, placedEntries } from '../files/carried-menu';
import { FileLinkContext, useFileLink, type FileLinkScope } from '../files/file-links';
import { fileMenuEntries } from '../files/file-menu';
import { outsidePath } from '../files/file-paths';
import { mentionedEntry } from '../files/file-sources';
import { localTarget } from '../files/local-links';
import { useOutsideConsent } from '../files/outside-consent';
import { useContextMenu } from '../ui/useContextMenu';

/**
 * Calls whose target is a file: the file tools, a download's file, the video a media call read,
 * and the page the built-in browser opened from a file.
 */
const FILE_TARGETS = new Set([
  'fs.read',
  'fs.write',
  'fs.patch',
  'http.download',
  'media.frames',
  'media.probe',
  'browser.open',
]);

/** The row line of a path the projection cut short: the path it names is not the file's. */
const CUT = '…';

/**
 * A file outside the workspace that a call's row names, by its full path. The first opening in
 * the conversation asks; Core opens it only once it finds the path in that row.
 */
function OutsideFileButton({
  path,
  shown,
  className,
  call,
  scope,
}: {
  path: string;
  shown: string;
  className: string;
  call: string;
  scope: FileLinkScope;
}) {
  const menu = useContextMenu();
  const consent = useOutsideConsent(scope.session, path);
  const from = { kind: 'tool_call', tool_call_id: call } as const;
  const entry = mentionedEntry(from, path);
  const target = { session_id: scope.session, mentioned_path: path, mentioned_in: from };
  const label = `打开 ${path}`;
  return (
    <>
      <button
        aria-label={label}
        className={`${className} agent-file-link`}
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          consent.ask(() => openPlaced(scope, entry, target, path));
        }}
        onContextMenu={(event) =>
          menu.open(event, `${path} 的操作`, placedEntries(scope, entry, target, path, consent.ask))
        }
        title={label}
        type="button"
      >
        {shown}
      </button>
      {menu.menu}
      {consent.dialog}
    </>
  );
}

/**
 * A file named in a call's row. A workspace file opens in the right sidebar (or outside Aura, as
 * a path in a message does); a file outside the workspace named by its full path opens the same
 * way once the user agrees. The right-click menu has the rest. Anything else stays text.
 */
function FileButton({
  text,
  className,
  shown = text,
  call,
}: {
  text: string;
  className: string;
  shown?: string;
  call: string;
}) {
  const link = useFileLink(text, { file: true });
  const scope = useContext(FileLinkContext);
  const menu = useContextMenu();
  // A search's match names its line too; the file is the path before it.
  const outside =
    !link && scope
      ? outsidePath(text.replace(/:\d+$/u, ''), scope.root, { file: true })
      : undefined;
  if (outside && scope)
    return (
      <OutsideFileButton
        call={call}
        className={className}
        path={outside.replaceAll('/', '\\')}
        scope={scope}
        shown={shown}
      />
    );
  if (!link)
    return (
      <span className={className} title={shown}>
        {shown}
      </span>
    );
  const label = `打开 ${link.reference.path}${link.reference.line ? `:${link.reference.line}` : ''}`;
  return (
    <>
      <button
        aria-label={label}
        className={`${className} agent-file-link`}
        // Inside a row that expands: open the file, not the row.
        onClick={(event) => {
          event.preventDefault();
          event.stopPropagation();
          link.open();
        }}
        onContextMenu={(event) =>
          menu.open(
            event,
            `${link.reference.path} 的操作`,
            fileMenuEntries(link.scope, link.reference.path, 'conversation'),
          )
        }
        title={label}
        type="button"
      >
        {shown}
      </button>
      {menu.menu}
    </>
  );
}

/**
 * The file a call names, in its row's chip. A browser call's row names the page it showed by its
 * address once the call is done, and a page from a file has a file address: the address without
 * its query and fragment is the file.
 */
export function TargetLabel({
  tool,
  target,
  call,
}: {
  tool: string;
  target: string;
  call: string;
}) {
  const page = /^file:/iu.test(target) ? localTarget(target.replace(/[?#].*$/su, '')) : undefined;
  if (target.endsWith(CUT) || (!page && !FILE_TARGETS.has(tool)))
    return (
      <span className="agent-file-label" title={target}>
        {target}
      </span>
    );
  return (
    <FileButton call={call} className="agent-file-label" shown={target} text={page ?? target} />
  );
}

/** `path:line · excerpt`, as a search match reads in its row. */
const MATCH = /^(.+?):(\d+) · (.*)$/su;

/**
 * One line of a call's expanded row: what a lookup found and where a search matched open as
 * files; every other line is text.
 */
export function DetailLine({ tool, line, call }: { tool: string; line: string; call: string }) {
  if (tool === 'fs.find' && !line.endsWith(CUT))
    return <FileButton call={call} className="agent-detail-file" text={line} />;
  const match = tool === 'fs.search' ? MATCH.exec(line) : null;
  if (match && !match[1]!.endsWith(CUT))
    return (
      <>
        <FileButton
          call={call}
          className="agent-detail-file"
          shown={`${match[1]}:${match[2]}`}
          text={`${match[1]}:${match[2]}`}
        />
        {` · ${match[3]}`}
      </>
    );
  return <>{line}</>;
}
