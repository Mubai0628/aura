import { expect, test } from 'vitest';
import { binaryKind } from './file-kind';

// The file view showed Core's words for the model: 「ZIP-compatible archive」, 「unknown binary or
// text data」.
test('a file the viewer cannot show is named in Chinese by what it is', () => {
  expect(binaryKind('assets.zip', 'application/zip')).toBe('ZIP 压缩包');
  expect(binaryKind('setup.exe', 'application/vnd.microsoft.portable-executable')).toBe(
    'Windows 程序',
  );
  expect(binaryKind('lib/x.dll', 'application/vnd.microsoft.portable-executable')).toBe(
    'Windows 程序库',
  );
  expect(binaryKind('docs\\manual.pdf', 'application/pdf')).toBe('PDF 文档');
  expect(binaryKind('data.bin', 'application/octet-stream')).toBe('未知类型的文件');
});

test('a document that is a ZIP archive inside is named as the document', () => {
  expect(binaryKind('report.docx', 'application/zip')).toBe('Word 文档');
  expect(binaryKind('Sheet.XLSX', 'application/zip')).toBe('Excel 表格');
  // Its first bytes outrank a misleading extension.
  expect(binaryKind('fake.docx', 'application/pdf')).toBe('PDF 文档');
});

test('without a media type the extension names it, else it is of unknown type', () => {
  expect(binaryKind('backup.7z')).toBe('7z 压缩包');
  expect(binaryKind('old.doc', 'application/octet-stream')).toBe('Word 文档');
  expect(binaryKind('noextension')).toBe('未知类型的文件');
  expect(binaryKind('.gitkeep')).toBe('未知类型的文件');
});
