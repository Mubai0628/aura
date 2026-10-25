import type { AgentAttachmentDraft } from '@aura/contracts/generated';

/**
 * A picture or file added to the
 * composer. A picture is sent as a PNG or JPEG (others are redrawn as one, large ones scaled
 * down); a text file of up to 256 KB is sent with its contents; any other file is named by its
 * place. Core checks its final path and identity, and grants this session a read of an external
 * single file when the user sends it.
 */
export type ComposerAttachment =
  | {
      id: string;
      kind: 'image';
      name: string;
      mediaType: 'image/png' | 'image/jpeg';
      dataBase64: string;
      bytes: number;
    }
  /** `path`, where a text file the user picked or dropped is, to open it from its card. */
  | { id: string; kind: 'text'; name: string; text: string; bytes: number; path?: string }
  | { id: string; kind: 'file'; name: string; path: string; bytes: number };

export const MAX_ATTACHMENTS = 10;
export const MAX_TEXT_BYTES = 256 * 1024;
export const MAX_IMAGE_BYTES = 3 * 1024 * 1024;
export const MAX_ATTACHED_BYTES = 4 * 1024 * 1024;
/** The longest side a picture is sent at; larger ones are scaled down. */
export const MAX_IMAGE_SIDE = 2048;

const TEXT_EXTENSIONS = new Set(
  (
    'txt md markdown csv tsv json jsonc json5 yaml yml toml ini cfg conf env log xml html htm css ' +
    'scss less js mjs cjs jsx ts tsx vue svelte py rs go java kt kts swift c h cc cpp hpp cs php rb ' +
    'lua pl r sql sh bash zsh ps1 psm1 bat cmd gradle properties dockerfile makefile gitignore ' +
    'editorconfig svg tex rst adoc org srt vtt'
  ).split(' '),
);

/** The extension of a file name, lower case, without the dot ("" when it has none). */
export function extension(name: string): string {
  const dot = name.lastIndexOf('.');
  return dot > 0 ? name.slice(dot + 1).toLowerCase() : name.toLowerCase();
}

const isPicture = (file: File) => /^image\/(png|jpeg|gif|webp|bmp)$/u.test(file.type);

/** Whether the file reads as text: its type says so, or its extension is one of a text file. */
export function isTextFile(file: File): boolean {
  if (/^text\//u.test(file.type)) return true;
  if (/(json|xml|javascript|typescript|yaml|toml|x-sh)/u.test(file.type)) return true;
  return TEXT_EXTENSIONS.has(extension(file.name));
}

const id = () => `att-${Math.random().toString(36).slice(2)}-${Date.now().toString(36)}`;

function base64(bytes: Uint8Array): string {
  let binary = '';
  for (let at = 0; at < bytes.length; at += 0x8000)
    binary += String.fromCharCode(...bytes.subarray(at, at + 0x8000));
  return btoa(binary);
}

/** A picture redrawn at most MAX_IMAGE_SIDE on its longest side, as PNG or JPEG. */
export type PictureEncoder = (
  file: File,
) => Promise<{ mediaType: 'image/png' | 'image/jpeg'; bytes: Uint8Array } | undefined>;

/** The browser's own decoder and canvas; a picture it cannot decode is undefined. */
export const encodePicture: PictureEncoder = async (file) => {
  const original = new Uint8Array(await file.arrayBuffer());
  let bitmap: ImageBitmap;
  try {
    bitmap = await createImageBitmap(file);
  } catch {
    return undefined;
  }
  const scale = Math.min(1, MAX_IMAGE_SIDE / Math.max(bitmap.width, bitmap.height));
  const kept = file.type === 'image/png' || file.type === 'image/jpeg';
  if (kept && scale === 1 && original.length <= MAX_IMAGE_BYTES) {
    bitmap.close();
    return { mediaType: file.type as 'image/png' | 'image/jpeg', bytes: original };
  }
  const canvas = document.createElement('canvas');
  canvas.width = Math.max(1, Math.round(bitmap.width * scale));
  canvas.height = Math.max(1, Math.round(bitmap.height * scale));
  canvas.getContext('2d')?.drawImage(bitmap, 0, 0, canvas.width, canvas.height);
  bitmap.close();
  const encode = (type: string, quality?: number) =>
    new Promise<Blob | null>((resolve) => canvas.toBlob(resolve, type, quality));
  // A PNG stays one while it fits (it may be a screenshot with sharp text); otherwise JPEG.
  if (file.type === 'image/png' || file.type === 'image/gif' || file.type === 'image/bmp') {
    const png = await encode('image/png');
    if (png && png.size <= MAX_IMAGE_BYTES)
      return { mediaType: 'image/png', bytes: new Uint8Array(await png.arrayBuffer()) };
  }
  for (const quality of [0.9, 0.8, 0.65]) {
    const jpeg = await encode('image/jpeg', quality);
    if (jpeg && jpeg.size <= MAX_IMAGE_BYTES)
      return { mediaType: 'image/jpeg', bytes: new Uint8Array(await jpeg.arrayBuffer()) };
  }
  return undefined;
};

/**
 * One added file as the composer keeps it, or why it cannot be sent. `place` is where the file
 * is on this computer (empty for a pasted picture).
 */
export async function readAttachment(
  file: File,
  place: (file: File) => string,
  encode: PictureEncoder = encodePicture,
): Promise<ComposerAttachment | { error: string }> {
  const name = file.name || (isPicture(file) ? '粘贴的图片.png' : '文件');
  if (isPicture(file)) {
    const picture = await encode(file);
    if (!picture) return { error: `「${name}」无法作为图片读取。` };
    return {
      id: id(),
      kind: 'image',
      name,
      mediaType: picture.mediaType,
      dataBase64: base64(picture.bytes),
      bytes: picture.bytes.length,
    };
  }
  if (isTextFile(file) && file.size <= MAX_TEXT_BYTES) {
    const text = await file.text();
    // A file that only looks like text (a NUL byte, or bytes that are not UTF-8) is named instead.
    if (!text.includes('\u0000') && !text.includes('�')) {
      const path = place(file);
      return {
        id: id(),
        kind: 'text',
        name,
        text,
        bytes: new TextEncoder().encode(text).length,
        ...(path ? { path } : {}),
      };
    }
  }
  const path = place(file);
  if (!path) return { error: `「${name}」没有可用的位置，无法附加。` };
  return { id: id(), kind: 'file', name, path, bytes: file.size };
}

/** What the request carries for the attachments. */
export function attachmentDrafts(
  attachments: readonly ComposerAttachment[],
): AgentAttachmentDraft[] {
  return attachments.map((attachment) => {
    switch (attachment.kind) {
      case 'image':
        return {
          kind: 'image',
          name: attachment.name,
          media_type: attachment.mediaType,
          data_base64: attachment.dataBase64,
        };
      case 'text':
        return { kind: 'text', name: attachment.name, text: attachment.text };
      case 'file':
        return { kind: 'file', path: attachment.path };
    }
  });
}

/** The kinds of file a card's icon tells apart, each with its own colour. */
export type FileKind =
  | 'pdf'
  | 'doc'
  | 'sheet'
  | 'slides'
  | 'archive'
  | 'code'
  | 'text'
  | 'image'
  | 'audio'
  | 'video'
  | 'other';

const KINDS: [FileKind, string][] = [
  ['pdf', 'pdf'],
  ['doc', 'doc docx odt rtf pages wps'],
  ['sheet', 'xls xlsx ods csv tsv numbers et'],
  ['slides', 'ppt pptx odp key dps'],
  ['archive', 'zip 7z rar tar gz tgz bz2 xz'],
  [
    'code',
    'js mjs cjs jsx ts tsx py rs go java kt swift c h cc cpp hpp cs php rb lua sql sh bash ps1 ' +
      'psm1 bat cmd json jsonc yaml yml toml xml html htm css scss less vue svelte ini',
  ],
  ['text', 'txt md markdown log rst adoc org srt vtt tex'],
  ['image', 'png jpg jpeg gif webp bmp svg ico tif tiff heic avif'],
  ['audio', 'mp3 wav flac ogg oga m4a aac opus wma'],
  ['video', 'mp4 m4v webm ogv mov mkv avi wmv flv'],
];

export function fileKind(name: string): FileKind {
  const ext = extension(name);
  return KINDS.find(([, list]) => list.split(' ').includes(ext))?.[0] ?? 'other';
}

/** The type a card names under the file: its extension, or 文件. */
export function typeLabel(name: string): string {
  const ext = extension(name);
  return ext && ext !== name.toLowerCase() ? ext.toUpperCase() : '文件';
}

/** Bytes as the cards show them. */
export function sizeLabel(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.ceil(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
