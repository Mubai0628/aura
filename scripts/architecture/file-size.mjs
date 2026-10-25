import { existsSync, readFileSync } from 'node:fs';
import { extname } from 'node:path';
import { capture } from './capture.mjs';

const sourceExtensions = new Set([
  '.rs',
  '.ts',
  '.tsx',
  '.cts',
  '.mts',
  '.css',
  '.html',
  '.c',
  '.cpp',
  '.h',
  '.hpp',
  '.js',
  '.jsx',
  '.mjs',
  '.cjs',
  '.py',
  '.ps1',
  '.sh',
  '.sql',
]);

export function handwrittenSource(path) {
  return sourceExtensions.has(extname(path)) && !path.split('/').includes('generated');
}

// Count physical source lines, including tests, comments and blank lines. CRLF and LF agree.
export function lineCount(source) {
  if (!source.length) return 0;
  return source.split('\n').length - Number(source.endsWith('\n'));
}

export function sourceFiles(root) {
  return [
    ...new Set(
      capture(root, 'git', ['ls-files', '-co', '--exclude-standard', '-z'])
        .toString('utf8')
        .split('\0')
        .filter((path) => handwrittenSource(path) && existsSync(`${root}/${path}`)),
    ),
  ].sort();
}

export function sizeViolations(files, limits, read) {
  return files.flatMap((path) => {
    const lines = lineCount(read(path));
    const ceiling = limits[path] ?? 1000;
    return lines > ceiling ? [{ path, lines, ceiling }] : [];
  });
}

export function checkFileSizes(root, baseline) {
  const files = sourceFiles(root);
  const violations = sizeViolations(files, baseline.ceilings, (path) =>
    readFileSync(`${root}/${path}`, 'utf8'),
  );
  if (violations.length) {
    throw new Error(
      `handwritten file size guard failed:\n${violations
        .map(({ path, lines, ceiling }) => `${path}: ${lines} lines exceeds ${ceiling}`)
        .join('\n')}`,
    );
  }
  console.log(
    `handwritten file size guard passed (${files.length} files; maximum 1000 for new files)`,
  );
}
