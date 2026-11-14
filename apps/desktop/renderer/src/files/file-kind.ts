/**
 * The kind of a file the viewer cannot show, in the words a person uses. Core names the kind by
 * the file's first bytes, in English, for the model; here it is named by its media type, and an
 * archive that is a document by its extension. Without either, it is a file of unknown type.
 */

/** Documents and packages that are ZIP archives inside. */
const ZIP_DOCUMENTS: Record<string, string> = {
  docx: 'Word 文档',
  xlsx: 'Excel 表格',
  pptx: 'PowerPoint 演示文稿',
  odt: 'OpenDocument 文档',
  ods: 'OpenDocument 表格',
  odp: 'OpenDocument 演示文稿',
  epub: 'EPUB 电子书',
  jar: 'Java 程序包',
  apk: 'Android 安装包',
  vsix: 'VS Code 扩展包',
  whl: 'Python 安装包',
  nupkg: 'NuGet 程序包',
};

const BY_MEDIA: Record<string, string> = {
  'application/zip': 'ZIP 压缩包',
  'application/pdf': 'PDF 文档',
  'application/vnd.microsoft.portable-executable': 'Windows 程序',
  'application/x-elf': 'Linux 程序',
  'application/java-vm': 'Java 类文件',
  'image/png': 'PNG 图片',
  'image/jpeg': 'JPEG 图片',
  'image/gif': 'GIF 图片',
  'image/webp': 'WebP 图片',
  'image/svg+xml': 'SVG 图片',
  'audio/mpeg': 'MP3 音频',
  'audio/wav': 'WAV 音频',
  'video/mp4': 'MP4 视频',
  'video/webm': 'WebM 视频',
};

/** By extension, when the first bytes did not say. */
const BY_EXTENSION: Record<string, string> = {
  ...ZIP_DOCUMENTS,
  zip: 'ZIP 压缩包',
  '7z': '7z 压缩包',
  rar: 'RAR 压缩包',
  gz: 'gzip 压缩包',
  tgz: 'gzip 压缩包',
  tar: 'tar 归档',
  pdf: 'PDF 文档',
  exe: 'Windows 程序',
  com: 'Windows 程序',
  scr: 'Windows 程序',
  dll: 'Windows 程序库',
  msi: 'Windows 安装包',
  doc: 'Word 文档',
  xls: 'Excel 表格',
  ppt: 'PowerPoint 演示文稿',
  iso: '光盘映像',
  db: '数据库文件',
  sqlite: '数据库文件',
  ttf: '字体文件',
  otf: '字体文件',
  woff: '字体文件',
  woff2: '字体文件',
  class: 'Java 类文件',
  pyc: 'Python 字节码',
};

function extensionOf(name: string): string | undefined {
  const base = name.split(/[\\/]/u).pop() ?? name;
  const dot = base.lastIndexOf('.');
  return dot > 0 ? base.slice(dot + 1).toLowerCase() : undefined;
}

/** The kind of file `name` is, from the media type Core read in its first bytes when there is one. */
export function binaryKind(name: string, mediaType = ''): string {
  const extension = extensionOf(name);
  if (mediaType === 'application/zip' && extension && ZIP_DOCUMENTS[extension])
    return ZIP_DOCUMENTS[extension];
  if (mediaType === 'application/vnd.microsoft.portable-executable' && extension === 'dll')
    return 'Windows 程序库';
  return (
    BY_MEDIA[mediaType] ?? (extension ? BY_EXTENSION[extension] : undefined) ?? '未知类型的文件'
  );
}
