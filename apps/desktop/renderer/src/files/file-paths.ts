/**
 * Which text names a file of the conversation's workspace. A path in a
 * message opens the file, so a false match would turn ordinary code (`foo.bar()`, `1.2.3`) into a
 * link: a name counts only with an extension files have, or a name files are known by.
 */

export interface FileReference {
  /** Workspace-relative, with `/` between parts. */
  path: string;
  line?: number;
}

/**
 * Kinds the sidebar cannot show; a click opens them outside Aura. Video and sound its player plays are shown; mkv and avi stay outside.
 */
const OUTSIDE =
  'doc docx xls xlsx ppt pptx odt ods odp rtf zip 7z rar tar gz tgz glb gltf fbx stl blend mkv ' +
  'avi psd ttf otf woff woff2 exe msi';
const NOT_SHOWN = new Set(`wasm pdf ${OUTSIDE}`.split(' '));
const VIDEOS = new Set('mp4 m4v webm ogv mov'.split(' '));
const SOUNDS = new Set('mp3 wav flac ogg oga m4a aac opus'.split(' '));

// Not `diff` or `patch`: Aura's own tool names (`git.diff`, `fs.patch`) read the same way.
const EXTENSIONS = new Set(
  (
    'md mdx markdown txt rst adoc rs ts tsx mts cts js jsx mjs cjs json jsonc json5 toml yaml yml ' +
    'ini cfg conf env properties lock css scss sass less html htm xml svg png jpg jpeg gif webp bmp ' +
    'ico avif py pyi go java kt kts scala c h cc cpp cxx hpp hh cs swift rb php pl lua dart jl ex ' +
    'exs erl hs ml zig nim sh bash zsh fish ps1 psm1 psd1 bat cmd sql graphql gql proto vue svelte ' +
    'astro gradle cmake mk tf hcl nix csv tsv log wasm pdf ipynb ' +
    // Arduino sketches and firmware images.
    'ino pde elf hex ' +
    `${[...VIDEOS, ...SOUNDS].join(' ')} ` +
    // Kinds that open outside Aura (documents, archives, models, fonts, programs).
    OUTSIDE
  ).split(' '),
);

const NAMES = new Set(
  (
    'Dockerfile Makefile Containerfile Justfile Procfile Gemfile Rakefile Vagrantfile LICENSE ' +
    'LICENCE NOTICE README CHANGELOG AUTHORS CODEOWNERS .gitignore .gitattributes .gitmodules ' +
    '.editorconfig .npmrc .nvmrc .node-version .prettierrc .prettierignore .eslintrc .env ' +
    '.dockerignore'
  ).split(' '),
);

const IMAGES = new Set('png jpg jpeg gif webp bmp ico avif svg'.split(' '));
const MARKDOWN = new Set('md mdx markdown'.split(' '));

/** Characters a path in prose never holds; quotes, brackets and spaces end it. */
const NOT_A_PATH = /[\s'"`<>()[\]{}*?|,;!=+^$%&]/u;

function extensionOf(name: string): string | undefined {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : undefined;
}

export function basename(path: string): string {
  return path.split('/').at(-1) ?? path;
}

export function isMarkdownPath(path: string): boolean {
  return MARKDOWN.has(extensionOf(basename(path)) ?? '');
}

export function isImagePath(path: string): boolean {
  return IMAGES.has(extensionOf(basename(path)) ?? '');
}

/** What the 文件 tab plays rather than reads: a picture, a video or a sound. */
export function mediaKind(path: string): 'image' | 'video' | 'audio' | undefined {
  const extension = extensionOf(basename(path)) ?? '';
  if (IMAGES.has(extension)) return 'image';
  if (VIDEOS.has(extension)) return 'video';
  return SOUNDS.has(extension) ? 'audio' : undefined;
}

/** The 文件 tab shows text, Markdown and pictures; other kinds open outside. */
export function sidebarShows(path: string): boolean {
  return !NOT_SHOWN.has(extensionOf(basename(path)) ?? '');
}

/** `path:12`, `path:12:5` or `path#L12`: the line, and the path without it. */
function splitLine(text: string): { path: string; line?: number } {
  const anchor = /^(.*)#L(\d+)$/u.exec(text);
  if (anchor) return { path: anchor[1]!, line: Number(anchor[2]) };
  const position = /^(.*?):(\d+)(?::\d+)?$/u.exec(text);
  if (position && !/^[a-z]$/iu.test(position[1]!))
    return { path: position[1]!, line: Number(position[2]) };
  return { path: text };
}

/** Characters a Windows path cannot hold after its drive, and line breaks. */
const NOT_A_WINDOWS_PATH = /[\n\r\t"<>|?*:]/u;

/**
 * A file outside the
 * workspace named by its full path, a drive first, with a name this conversation recognizes as a
 * file's; spaces and brackets are allowed, as in `C:\Program Files (x86)\…`. Nothing for a path in
 * the workspace (that is `fileReference`'s) or for anything else.
 */
export function outsidePath(text: string, root: string): string | undefined {
  const path = text.trim();
  if (!/^[a-z]:[\\/]/iu.test(path) || path.length > 400 || NOT_A_WINDOWS_PATH.test(path.slice(2)))
    return undefined;
  const base = root.replaceAll('\\', '/').replace(/\/+$/u, '').toLowerCase();
  const spelled = path.replaceAll('\\', '/').toLowerCase();
  if (base && spelled.startsWith(`${base}/`)) return undefined;
  const parts = path.slice(3).split(/[\\/]/u);
  if (parts.some((part) => part === '' || part === '.' || part === '..')) return undefined;
  const name = parts.at(-1)!;
  if (!NAMES.has(name) && !EXTENSIONS.has(extensionOf(name) ?? '')) return undefined;
  return path;
}

/**
 * The workspace file a piece of text names, if any. An absolute path counts only inside the
 * workspace root (Windows paths compare without case). `file: true` is for text known to name a
 * file — a file tool's target — which needs no recognisable extension.
 */
export function fileReference(
  text: string,
  root: string,
  options: { file?: boolean } = {},
): FileReference | undefined {
  const trimmed = text.trim();
  if (!trimmed || trimmed.length > 400 || NOT_A_PATH.test(trimmed)) return undefined;
  const { path: spelled, line } = splitLine(trimmed);
  let path = spelled.replaceAll('\\', '/');
  if (/^[a-z]:\//iu.test(path) || path.startsWith('/')) {
    const base = root.replaceAll('\\', '/').replace(/\/+$/u, '');
    if (!base || path.length <= base.length + 1) return undefined;
    if (path.slice(0, base.length + 1).toLowerCase() !== `${base.toLowerCase()}/`) return undefined;
    path = path.slice(base.length + 1);
  }
  path = path.replace(/^(?:\.\/)+/u, '');
  const parts = path.split('/');
  if (parts.some((part) => part === '' || part === '.' || part === '..')) return undefined;
  const name = parts.at(-1)!;
  if (!options.file) {
    const extension = extensionOf(name);
    if (!NAMES.has(name) && !EXTENSIONS.has(extension ?? '')) return undefined;
  }
  return line === undefined || line < 1 ? { path } : { path, line };
}
