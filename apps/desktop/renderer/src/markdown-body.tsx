import { useContext, type ComponentPropsWithoutRef } from 'react';
import { Streamdown, defaultRehypePlugins, defaultRemarkPlugins } from 'streamdown';
import type { Components, ExtraProps, StreamdownProps } from 'streamdown';
import { FileLinkContext, useFileLink } from './files/file-links';
import type { FileReference } from './files/file-paths';
import { remarkLocalFileLinks } from './files/local-links';
import { remarkAddresses, sameAddress, textAddress } from './markdown-addresses';
import { openPlaced, placedEntries } from './files/carried-menu';
import { fileMenuEntries } from './files/file-menu';
import { outsidePath } from './files/file-paths';
import { mentionedEntry } from './files/file-sources';
import { MentionContext } from './files/mention-context';
import { useContextMenu } from './ui/useContextMenu';
import { openWebLink, webAddress, webLinkEntries } from './web-links';
import './markdown.css';

/**
 * Markdown rendering for the bodies the product renders as Markdown: the user's
 * own messages, the model's final answer and, while a step is still running, the
 * process text that the product shows as prose. Reasoning and tool briefs stay
 * plain text and never come through here.
 *
 * The renderer is `streamdown`, chosen by measuring it against the requirements
 * rather than by preference:
 *
 * - It is built for Markdown that arrives in pieces. It exposes
 *   `parseMarkdownIntoBlocks`, memoizes each finished block, and can parse an
 *   unterminated construct, which is exactly the shape of a streaming answer.
 * - It supports GFM (tables, strikethrough, autolinks) and CJK text, both of which
 *   this product needs.
 *
 * Three defaults are deliberately changed:
 *
 * - The raw-HTML pass is removed. With `rehype-raw` gone, HTML written in the
 *   source cannot become elements at all; `sanitize` and `harden` still run, and
 *   `skipHtml` stays as a second line of defence. Removing `raw` is what makes
 *   the guarantee hold: `skipHtml` alone cannot recognise source HTML that the
 *   raw pass has already turned into nodes.
 * - `animated` would stagger words to fake a typing effect. The product shows what
 *   it has as soon as it has it, so no delay is introduced.
 * - `controls` adds per-block copy/download/fullscreen toolbars. The surrounding
 *   conversation already owns selection and copying, so these would duplicate it.
 *
 * `mode` follows the caller: `streaming` while the body is still arriving, so a
 * half-written fence or table renders as it settles instead of breaking, and
 * `static` for a body that is already complete.
 */
export function MarkdownBody({
  text,
  className,
  streaming = false,
}: {
  text: string;
  className?: string;
  streaming?: boolean;
}) {
  return <StreamdownHost text={text} className={className} streaming={streaming} />;
}

/**
 * The same renderer for a caller that owns the surrounding element and its own
 * loading state. It exists so the shared body loader can stay responsible for
 * fetching text while this module stays responsible for rendering it.
 */
export function MarkdownBlocks({
  text,
  className,
  streaming = false,
}: {
  text: string;
  className?: string;
  streaming?: boolean;
}) {
  return <StreamdownHost text={text} className={className} streaming={streaming} />;
}

function StreamdownHost({
  text,
  className,
  streaming,
}: {
  text: string;
  className?: string | undefined;
  streaming: boolean;
}) {
  const classes = className ? `markdown-body ${className}` : 'markdown-body';
  return (
    <div className={classes} data-markdown-source="streamdown">
      <Streamdown
        mode={streaming ? 'streaming' : 'static'}
        dir="auto"
        // No typing animation: an available body is shown immediately.
        animated={false}
        // The conversation owns selection and copying, so per-block toolbars would
        // duplicate it.
        controls={false}
        lineNumbers={false}
        // Sanitize and harden only. The raw pass is left out so that HTML in the
        // source stays text, which `skipHtml` then also covers.
        rehypePlugins={REHYPE_PLUGINS}
        remarkPlugins={REMARK_PLUGINS}
        // `a` is the one element whose rendering is ours: a link shows its text
        // and its full address, and stays a plain span rather than a control this
        // desktop cannot honour.
        components={COMPONENTS}
        skipHtml
      >
        {text}
      </Streamdown>
    </div>
  );
}

/** The installed renderer's own cleanup passes, without its raw-HTML pass.
 *  Defined once so every body shares one reference and its block memoization is
 *  not invalidated by a new array on each render. */
const REHYPE_PLUGINS = [defaultRehypePlugins.sanitize, hardenTextOnly()].filter(
  (plugin) => plugin !== undefined,
) as NonNullable<StreamdownProps['rehypePlugins']>;

/** The renderer's own Markdown passes, then links to local files become file paths
 *  ([`remarkLocalFileLinks`]) before the link guard sees their addresses. Then web
 *  addresses end where Chinese punctuation begins, and emails stay text ([`remarkAddresses`]). */
const REMARK_PLUGINS = [
  ...Object.values(defaultRemarkPlugins),
  remarkLocalFileLinks,
  remarkAddresses,
] as NonNullable<StreamdownProps['remarkPlugins']>;

/** An address the guard still refuses keeps its words and nothing else — not an English
 *  "[blocked]" after them. */
function hardenTextOnly() {
  const harden = defaultRehypePlugins.harden;
  if (!Array.isArray(harden)) return harden;
  const [plugin, options] = harden as [unknown, Record<string, unknown> | undefined];
  return [plugin, { ...options, linkBlockPolicy: 'text-only', imageBlockPolicy: 'text-only' }];
}

/** Only `a` and inline code are replaced; every other element keeps the renderer's own
 *  component. One module-level object, for the same reason as the plugin list. */
const COMPONENTS: Components = { a: VisibleLink, inlineCode: InlineCode };

/**
 * Inline code that names a file of the conversation's workspace opens it in
 * the right sidebar ("Click a file path in the conversation to open it"). Outside a conversation,
 * and for any other code, it stays code. A file the sidebar cannot show opens outside Aura
 * (a program or a script only in the file manager), and a right-click offers the rest.
 */
function InlineCode({ children, node, ...rest }: ComponentPropsWithoutRef<'code'> & ExtraProps) {
  const text = nodeText(node);
  const link = useFileLink(text);
  const menu = useContextMenu();
  const scope = useContext(FileLinkContext);
  // A web address written as code is a link too.
  const web = !link && scope ? textAddress(text.trim()) : undefined;
  // So is a file outside the
  // workspace named by its full path in a message.
  const mention = useContext(MentionContext);
  const placed = !link && !web && scope && mention ? outsidePath(text, scope.root) : undefined;
  // A file named by its absolute path reads as its place in the workspace.
  const shown = link && /^(?:[a-z]:[\\/]|\/)/iu.test(text) ? workspacePath(link.reference) : text;
  // The renderer's own marker stays: styles and tests find inline code by it.
  const code = (
    <code data-streamdown="inline-code" {...rest}>
      {shown === text ? children : shown}
    </code>
  );
  if (web && scope)
    return (
      <>
        <a
          className="markdown-web-link markdown-code-link"
          href={web}
          onAuxClick={(event) => {
            event.preventDefault();
            if (event.button === 1) void openWebLink(scope, web);
          }}
          onClick={(event) => {
            event.preventDefault();
            void openWebLink(scope, web);
          }}
          onContextMenu={(event) => menu.open(event, '链接的操作', webLinkEntries(scope, web))}
          title={`在默认浏览器中打开 ${web}`}
        >
          {code}
        </a>
        {menu.menu}
      </>
    );
  if (placed && scope && mention) {
    const entry = mentionedEntry(mention, placed);
    const target = { session_id: scope.session, mentioned_path: placed, mentioned_in: mention };
    return (
      <>
        <button
          aria-label={`打开 ${placed}`}
          className="markdown-file-link"
          onClick={() => openPlaced(scope, entry, target, placed)}
          onContextMenu={(event) =>
            menu.open(event, `${placed} 的操作`, placedEntries(scope, entry, target, placed))
          }
          title={`打开 ${placed}`}
          type="button"
        >
          {code}
        </button>
        {menu.menu}
      </>
    );
  }
  if (!link) return code;
  return (
    <>
      <button
        aria-label={`打开 ${shown}`}
        className="markdown-file-link"
        onClick={link.open}
        onContextMenu={(event) =>
          menu.open(
            event,
            `${link.reference.path} 的操作`,
            fileMenuEntries(link.scope, link.reference.path, 'conversation'),
          )
        }
        title={`打开 ${shown}`}
        type="button"
      >
        {code}
      </button>
      {menu.menu}
    </>
  );
}

function workspacePath(reference: FileReference): string {
  return reference.line === undefined ? reference.path : `${reference.path}:${reference.line}`;
}

/** The placeholder Streamdown substitutes for a link whose target has not
 *  arrived yet. It is an internal marker, never an address to show. */
const INCOMPLETE_LINK = 'streamdown:incomplete-link';

/**
 * A link: the label the model wrote, followed by the address in parentheses unless the label
 * already is that address, so where it leads is always in sight. The URL stays readable and
 * selectable inside a `bdi` that keeps its direction and lets it wrap.
 *
 * In a conversation an http(s) link opens in the
 * default browser — Main checks the address again — and a right-click can copy it. Other
 * addresses, and links outside a conversation, stay text.
 */
function VisibleLink({ children, href, node }: ComponentPropsWithoutRef<'a'> & ExtraProps) {
  const url = typeof href === 'string' && href !== INCOMPLETE_LINK ? href : '';
  // The label's own text decides whether the address would be a repetition.
  // Comparing rendered children instead would compare React nodes, not words.
  const label = nodeText(node);
  const scope = useContext(FileLinkContext);
  const menu = useContextMenu();
  const web = scope && url ? webAddress(url) : undefined;
  return (
    <span className="markdown-link">
      {scope && web ? (
        <a
          className="markdown-web-link"
          href={web}
          onAuxClick={(event) => {
            event.preventDefault();
            if (event.button === 1) void openWebLink(scope, web);
          }}
          onClick={(event) => {
            event.preventDefault();
            void openWebLink(scope, web);
          }}
          onContextMenu={(event) => menu.open(event, '链接的操作', webLinkEntries(scope, web))}
          title={`在默认浏览器中打开 ${web}`}
        >
          {children}
        </a>
      ) : (
        children
      )}
      {menu.menu}
      {url && !sameAddress(label, url) ? (
        <>
          （
          <bdi className="markdown-link-url" dir="ltr">
            {url}
          </bdi>
          ）
        </>
      ) : null}
    </span>
  );
}

/** The text inside a parsed node, without importing a tree type the renderer
 *  does not export. Only text nodes and their children are read. */
function nodeText(node: unknown): string {
  if (node === null || typeof node !== 'object') return '';
  const value = (node as { value?: unknown }).value;
  if (typeof value === 'string') return value;
  const children = (node as { children?: unknown }).children;
  return Array.isArray(children) ? children.map(nodeText).join('') : '';
}
