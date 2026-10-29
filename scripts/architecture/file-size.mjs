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

const testName = (name) => name === 'tests' || name.endsWith('_tests');

// Production source of a crate: not under a `tests` or `*_tests` directory, and not a `tests`,
// `*_tests` or `*test_support` file. Product files that only mention tests in their name count.
export function productionSource(path) {
  const parts = path.split('/');
  const stem = parts[parts.length - 1].replace(/\.[^.]+$/u, '');
  return (
    handwrittenSource(path) &&
    !parts.slice(0, -1).some(testName) &&
    !testName(stem) &&
    !stem.endsWith('test_support')
  );
}

// A capped crate's production lines under its `src` may shrink, never pass its ceiling.
export function crateViolations(files, ceilings, read) {
  return Object.entries(ceilings).flatMap(([crate, ceiling]) => {
    const lines = files
      .filter((path) => path.startsWith(`${crate}/src/`) && productionSource(path))
      .reduce((total, path) => total + lineCount(read(path)), 0);
    return lines > ceiling ? [{ crate, lines, ceiling }] : [];
  });
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
  const read = (path) => readFileSync(`${root}/${path}`, 'utf8');
  const violations = sizeViolations(files, baseline.ceilings, read);
  const crates = crateViolations(files, baseline.crate_ceilings?.ceilings ?? {}, read);
  if (violations.length || crates.length) {
    throw new Error(
      `handwritten file size guard failed:\n${[
        ...violations.map(
          ({ path, lines, ceiling }) => `${path}: ${lines} lines exceeds ${ceiling}`,
        ),
        ...crates.map(
          ({ crate, lines, ceiling }) =>
            `${crate}: ${lines} production lines exceeds ${ceiling}; new capabilities go into crates of their own (AGENTS.md)`,
        ),
      ].join('\n')}`,
    );
  }
  console.log(
    `handwritten file size guard passed (${files.length} files; maximum 1000 for new files; ` +
      `${Object.keys(baseline.crate_ceilings?.ceilings ?? {}).length} capped crate)`,
  );
}
