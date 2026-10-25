// Finds code comments that record how the code came about instead of why it is the way it is:
// planning-stage tags, dates, and who asked or reviewed. Such notes
// belong in commit messages and development documents; comments state technical reasons. P and
// R numbers (P04, R06) are not flagged: they name subsystems that exist in the code as modules
// and features (p04.rs, p05/, p08-contract-driver, xtask r06).
//
//   node scripts/release/comment-hygiene.mjs [folder]       report for a tree (default: this one)
import { readFileSync, readdirSync, statSync } from 'node:fs';
import { extname, join, relative, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';

const SLASH = new Set(['.rs', '.ts', '.tsx', '.mjs', '.cjs', '.js', '.css']);
const HASH = new Set(['.ps1', '.psm1', '.py', '.toml', '.yml', '.yaml', '.sh']);
const SKIPPED = new Set(['.git', 'node_modules', 'target', 'dist', 'generated']);

export const RULES = [
  // `#L12` is a line anchor, not a stage.
  ['stage', /(?<![\w#-])(?:S\d{2}(?:\.\d+)?[a-z]?|L\d{2}|L\d{1,2}\.\d+)(?![\w-])(?:\s+B\d+\b)?/u],
  // A period ends a sentence after a date; one followed by a word continues a name (`10-18.log`).
  ['date', /(?<![\w.-])(?:20\d\d-\d\d-\d\d|(?:09|10)-[0-3]\d)(?![\w-]|\.\w)/u],
  [
    'person',
    /\bowner(?:'s)?\s+(?:decision|retest|re-?test|test(?:ed|ing)?|report(?:ed)?|asked|said|chose|choice|wants?|request(?:ed)?|feedback|answer(?:ed)?|approv(?:ed|al)|confirm(?:ed)?|found|saw|noticed|desktop)\b|\(owner\b|\bowner\s+\d|所有者|原话|\bCodex\b|\bClaude\b(?!\s+Code)/u,
  ],
];

/** Comment lines of one source text, as [line number, text]. */
export function* comments(text, extension) {
  if (HASH.has(extension)) {
    const lines = text.split('\n');
    for (let index = 0; index < lines.length; index++) {
      const line = lines[index].trimStart();
      if (line.startsWith('#') && !line.startsWith('#!') && !line.startsWith('#[')) {
        yield [index + 1, line];
      }
    }
    return;
  }
  let index = 0;
  let line = 1;
  const length = text.length;
  while (index < length) {
    const char = text[index];
    if (char === '\n') {
      line += 1;
      index += 1;
    } else if (text.startsWith('//', index)) {
      let end = text.indexOf('\n', index);
      if (end < 0) end = length;
      yield [line, text.slice(index, end)];
      index = end;
    } else if (text.startsWith('/*', index)) {
      let end = text.indexOf('*/', index + 2);
      end = end < 0 ? length : end + 2;
      const parts = text.slice(index, end).split('\n');
      for (let offset = 0; offset < parts.length; offset++) yield [line + offset, parts[offset]];
      line += parts.length - 1;
      index = end;
    } else if (extension === '.rs' && char === "'") {
      // A Rust char literal ('"', '\n') is skipped whole; any other quote opens a lifetime.
      const literal = /^'(?:\\.|[^\\'\n])'/u.exec(text.slice(index, index + 12));
      index += literal ? literal[0].length : 1;
    } else if (char === '"' || char === "'" || char === '`') {
      // String literals are not comments, even when they look like one.
      index += 1;
      while (index < length && text[index] !== char) {
        if (text[index] === '\\') {
          // A string continued on the next line (`\` before the newline) still has that line.
          index += 1;
          if (text[index] === '\n') line += 1;
        } else if (text[index] === '\n') {
          // An apostrophe in JSX text is no string: the newline is still counted below.
          if (char !== '`') break;
          line += 1;
        }
        index += 1;
      }
      if (text[index] === char) index += 1;
    } else if (extension === '.rs' && char === 'r' && /^r#*"/u.test(text.slice(index, index + 4))) {
      const hashes = /^r(#*)"/u.exec(text.slice(index))[1];
      const close = `"${hashes}`;
      let end = text.indexOf(close, index + 2 + hashes.length);
      end = end < 0 ? length : end + close.length;
      line += (text.slice(index, end).match(/\n/gu) ?? []).length;
      index = end;
    } else {
      index += 1;
    }
  }
}

/** Findings in one file's text. */
export function findings(text, extension) {
  const out = [];
  for (const [line, comment] of comments(text, extension)) {
    // A code span names an identifier (a `S09` budget stage, a `p04_broker` module), not a note.
    const prose = comment.replace(/`[^`]*`/gu, '');
    const kinds = RULES.filter(([, rule]) => rule.test(prose)).map(([kind]) => kind);
    if (kinds.length) out.push({ line, kinds, text: comment.trim().slice(0, 200) });
  }
  return out;
}

function walk(folder, out) {
  for (const name of readdirSync(folder)) {
    if (SKIPPED.has(name) || (name.startsWith('.') && name !== '.github' && name !== '.cargo')) {
      continue;
    }
    const path = join(folder, name);
    let info;
    try {
      info = statSync(path);
    } catch {
      continue;
    }
    if (info.isDirectory()) walk(path, out);
    else if (SLASH.has(extname(name)) || HASH.has(extname(name))) out.push(path);
  }
  return out;
}

/** Findings for every source file under `root`, by path relative to it. */
export function scanTree(root) {
  const result = [];
  for (const path of walk(root, [])) {
    let text;
    try {
      text = readFileSync(path, 'utf8');
    } catch {
      continue;
    }
    const name = relative(root, path).split(sep).join('/');
    for (const finding of findings(text, extname(path))) result.push({ path: name, ...finding });
  }
  return result;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  const root = resolve(process.argv[2] ?? join(import.meta.dirname, '../..'));
  const result = scanTree(root);
  const files = new Set(result.map((finding) => finding.path));
  const kinds = {};
  for (const finding of result)
    for (const kind of finding.kinds) kinds[kind] = (kinds[kind] ?? 0) + 1;
  for (const finding of result)
    console.log(`${finding.path}:${finding.line} [${finding.kinds}] ${finding.text}`);
  console.log(JSON.stringify({ lines: result.length, files: files.size, kinds }));
  process.exitCode = result.length ? 1 : 0;
}
