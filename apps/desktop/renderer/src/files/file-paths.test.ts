import { expect, test } from 'vitest';
import { basename, fileReference, isImagePath, isMarkdownPath } from './file-paths';

// A file path in the conversation opens the file. Only text that names a
// file inside the conversation's workspace is a link; code that merely contains dots is not.

const root = 'C:\\Dev\\Demo';

test('workspace paths, with or without a line, are files to open', () => {
  expect(fileReference('src/main.rs', root)).toEqual({ path: 'src/main.rs' });
  expect(fileReference('src/main.rs:42', root)).toEqual({ path: 'src/main.rs', line: 42 });
  expect(fileReference('src/main.rs:42:7', root)).toEqual({ path: 'src/main.rs', line: 42 });
  expect(fileReference('src/main.rs#L9', root)).toEqual({ path: 'src/main.rs', line: 9 });
  expect(fileReference('./README.md', root)).toEqual({ path: 'README.md' });
  expect(fileReference('README.md', root)).toEqual({ path: 'README.md' });
  expect(fileReference('apps\\desktop\\package.json', root)).toEqual({
    path: 'apps/desktop/package.json',
  });
  expect(fileReference('.gitignore', root)).toEqual({ path: '.gitignore' });
  expect(fileReference('Dockerfile', root)).toEqual({ path: 'Dockerfile' });
});

test('an absolute path inside the workspace opens relative to it; one outside does not', () => {
  expect(fileReference('C:\\Dev\\Demo\\src\\main.rs', root)).toEqual({ path: 'src/main.rs' });
  expect(fileReference('c:/dev/demo/src/main.rs:3', root)).toEqual({
    path: 'src/main.rs',
    line: 3,
  });
  expect(fileReference('C:\\Dev\\Demo2\\main.rs', root)).toBeUndefined();
  expect(fileReference('C:\\Windows\\win.ini', root)).toBeUndefined();
  expect(fileReference('/etc/hosts', root)).toBeUndefined();
});

test('code, versions, commands and addresses are not paths', () => {
  for (const text of [
    'foo.bar()',
    'self.items',
    '1.2.3',
    'v1.0',
    'e.g.',
    'example.com',
    'npm install',
    'git.diff',
    '../outside.md',
    'src/../../x.rs',
    'a b.txt',
    'Option<String>',
    '',
    'src/',
  ])
    expect(fileReference(text, root), text).toBeUndefined();
});

test('a file tool names a file even without a known extension', () => {
  expect(fileReference('scripts/build', root, { file: true })).toEqual({ path: 'scripts/build' });
  expect(fileReference('scripts/build', root)).toBeUndefined();
});

test('names and kinds of files', () => {
  expect(basename('src/main.rs')).toBe('main.rs');
  expect(basename('README.md')).toBe('README.md');
  expect(isMarkdownPath('docs/Guide.MD')).toBe(true);
  expect(isMarkdownPath('notes.txt')).toBe(false);
  expect(isImagePath('assets/logo.svg')).toBe(true);
  expect(isImagePath('photo.JPEG')).toBe(true);
  expect(isImagePath('main.rs')).toBe(false);
});
