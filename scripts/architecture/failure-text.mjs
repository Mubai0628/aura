// A failure that crosses to the desktop reads `code: 中文句`: a dotted lowercase code, then a
// sentence in Chinese that says what happened and what to do. A bare code, English text or a
// debug dump reaches the window as a generic fallback. The crates that answer the desktop may not
// gain more of those; the files that still hold some are listed in failure-text-baseline.json with
// their count, which may only go down.
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative } from 'node:path';

/** Folders whose failures are answered to the desktop. */
export const FAILURE_TEXT_ROOTS = [
  'crates/aura-core/src/routes',
  'crates/aura-core/src/slash_commands',
  'crates/aura-extension-service/src',
  'crates/aura-plugin-session/src',
  'crates/aura-tool-service/src',
  'crates/aura-git-service/src',
  'crates/aura-package-service/src',
  'crates/aura-model-service/src',
  'crates/aura-workspace-service/src',
  'crates/aura-security-service/src',
];

/** The window strips a leading code whose first part is letters only (`CODE_PREFIX`). */
const CODE = /^(?:[a-z][a-z_]*|\{[a-z0-9_]*\})(?:\.(?:[a-z0-9_]+|\{[a-z0-9_]*\}))+: /u;
const CJK = /[㐀-鿿]/u;
/** Another error's own words, passed on unchanged: judged where they are written. */
const PASSED_ON = /^\{[a-z0-9_.]*\}$/u;

/** True when `text` is a failure a person can read: `code: 中文句`. */
export function readableFailure(text) {
  if (PASSED_ON.test(text)) return true;
  const code = CODE.exec(text);
  return code !== null && CJK.test(text.slice(code[0].length));
}

const TEST_FILE = /(?:^|[\\/])tests?[\\/]|(?:^|[\\/_])tests?\.rs$/u;

/** The Rust files under `folder`, product code only. */
function rustFiles(root, folder) {
  const found = [];
  const walk = (path) => {
    for (const name of readdirSync(path)) {
      const child = join(path, name);
      if (statSync(child).isDirectory()) walk(child);
      else if (name.endsWith('.rs')) {
        const shown = relative(root, child).replaceAll('\\', '/');
        if (!TEST_FILE.test(shown)) found.push(shown);
      }
    }
  };
  walk(join(root, folder));
  return found;
}

/**
 * Reads one Rust token at `at`: a string literal (with its text), a comment, a character literal,
 * or one character. Returns [end, text | undefined, kind].
 */
function token(source, at) {
  const two = source.slice(at, at + 2);
  if (two === '//') {
    const end = source.indexOf('\n', at);
    return [end < 0 ? source.length : end, undefined, 'comment'];
  }
  if (two === '/*') {
    let depth = 0;
    let index = at;
    while (index < source.length) {
      if (source.startsWith('/*', index)) {
        depth += 1;
        index += 2;
      } else if (source.startsWith('*/', index)) {
        depth -= 1;
        index += 2;
        if (depth === 0) break;
      } else index += 1;
    }
    return [index, undefined, 'comment'];
  }
  const raw = /^b?r(#*)"/u.exec(source.slice(at, at + 260));
  if (raw && !/[A-Za-z0-9_]/u.test(source[at - 1] ?? '')) {
    const close = `"${raw[1]}`;
    const start = at + raw[0].length;
    const end = source.indexOf(close, start);
    return [end + close.length, source.slice(start, end), 'string'];
  }
  if (source[at] === '"' || (source[at] === 'b' && source[at + 1] === '"')) {
    let index = source[at] === 'b' ? at + 2 : at + 1;
    let text = '';
    while (index < source.length && source[index] !== '"') {
      if (source[index] === '\\') {
        const next = source[index + 1];
        if (next === '\n' || next === '\r') {
          index += 1;
          while (/\s/u.test(source[index] ?? '')) index += 1;
          continue;
        }
        text += next === 'n' ? '\n' : next === 't' ? '\t' : next;
        index += 2;
      } else {
        text += source[index];
        index += 1;
      }
    }
    return [index + 1, text, 'string'];
  }
  const char = /^'(?:\\(?:u\{[0-9a-fA-F]+\}|x[0-9a-fA-F]{2}|.)|[^\\'\n])'/u.exec(
    source.slice(at, at + 12),
  );
  if (char) return [at + char[0].length, undefined, 'char'];
  return [at + 1, undefined, source[at]];
}

/**
 * The top-level arguments of the call whose `(` ends just before `at`. An argument that is one
 * string literal and nothing else carries it as `literal`; every argument carries its source as
 * `raw`.
 */
function argumentsAt(source, at) {
  const found = [];
  let depth = 0;
  let index = at;
  let start = at;
  let current = { literal: undefined, other: false };
  const close = (end) => found.push({ ...current, raw: source.slice(start, end) });
  while (index < source.length) {
    const [end, text, kind] = token(source, index);
    if (kind === 'string' && depth === 0) {
      if (current.literal === undefined && !current.other) current.literal = text;
      else current.other = true;
    } else if (kind === '(' || kind === '[' || kind === '{') {
      depth += 1;
      if (depth === 1) current.other = true;
    } else if (kind === ')' || kind === ']' || kind === '}') {
      if (depth === 0) {
        close(index);
        return found;
      }
      depth -= 1;
    } else if (kind === ',' && depth === 0) {
      close(index);
      start = end;
      current = { literal: undefined, other: false };
    } else if (depth === 0 && kind !== 'comment' && !/^\s$/u.test(kind ?? '')) {
      current.other = true;
    }
    index = end;
  }
  return found;
}

const CONSTANT =
  /\b(?:pub(?:\([a-z]+\))?\s+)?const\s+([A-Z][A-Z0-9_]*)\s*:\s*&(?:'static\s+)?str\s*=\s*/gu;
/** A text that begins as a failure does: a dotted code and a colon. */
const CODE_START = /^(?:[a-z][a-z0-9_]*|\{[a-z0-9_]*\})(?:\.(?:[a-z0-9_]+|\{[a-z0-9_]*\}))+:/u;

/** The `&str` constants written in `code`: name → { line, text }. */
function stringConstants(code) {
  const found = new Map();
  CONSTANT.lastIndex = 0;
  let match;
  while ((match = CONSTANT.exec(code)) !== null) {
    const [, text, kind] = token(code, match.index + match[0].length);
    if (kind === 'string')
      found.set(match[1], { line: code.slice(0, match.index).split('\n').length, text });
  }
  return found;
}

/** `source` with every `#[cfg(test)]` item blanked, line breaks kept. */
export function withoutTests(source) {
  let out = source;
  const marker = /#\[cfg\(test\)\]/gu;
  let match;
  while ((match = marker.exec(out)) !== null) {
    let index = match.index + match[0].length;
    // Further attributes on the same item.
    for (;;) {
      const gap = /^\s*/u.exec(out.slice(index))[0].length;
      if (!out.startsWith('#[', index + gap)) break;
      index = out.indexOf(']', index + gap) + 1;
    }
    let end = index;
    let depth = 0;
    while (end < out.length) {
      const [next, , kind] = token(out, end);
      end = next;
      if (kind === ';' && depth === 0) break;
      if (kind === '{') depth += 1;
      if (kind === '}') {
        depth -= 1;
        if (depth === 0) break;
      }
    }
    const blank = out.slice(match.index, end).replace(/[^\n]/gu, ' ');
    out = out.slice(0, match.index) + blank + out.slice(end);
    marker.lastIndex = match.index + 1;
  }
  return out;
}

/** Calls whose text becomes a failure's message, and which argument holds it. */
const CALLS = [
  [/\b(?:anyhow::)?(?:bail|anyhow)!\s*\(/gu, 0],
  [/\b(?:anyhow::)?ensure!\s*\(/gu, 1],
  [/#\[error\(/gu, 0],
  [/\.context\(/gu, 0],
];

/**
 * The failure texts in one file's product code that a person cannot read: [line, text][]. A call
 * that names a constant of the same file is judged by the constant's text, at the call. A constant
 * no such call uses is judged where it is written when its text begins with a code, and always in
 * a failures module (`failuresModule`), whose constants are all failure texts for other files.
 */
export function unreadableFailures(source, { failuresModule = false } = {}) {
  const code = withoutTests(source);
  const constants = stringConstants(code);
  const used = new Set();
  const found = [];
  for (const [pattern, position] of CALLS) {
    pattern.lastIndex = 0;
    let match;
    while ((match = pattern.exec(code)) !== null) {
      const argument = argumentsAt(code, match.index + match[0].length)[position];
      let text = argument?.literal;
      const name = /^\s*([A-Z][A-Z0-9_]*)\s*$/u.exec(argument?.raw ?? '')?.[1];
      if (text === undefined && name !== undefined && constants.has(name)) {
        used.add(name);
        text = constants.get(name).text;
      }
      if (text === undefined || readableFailure(text)) continue;
      found.push([code.slice(0, match.index).split('\n').length, text]);
    }
  }
  for (const [name, { line, text }] of constants) {
    if (used.has(name) || readableFailure(text)) continue;
    if (failuresModule || CODE_START.test(text)) found.push([line, text]);
  }
  return found.sort((a, b) => a[0] - b[0]);
}

/**
 * Files outside the roots that hold failure texts the roots' code answers with. A file whose name
 * ends in `failures.rs` is a failures module: every `&str` constant in it is a failure text.
 */
export const FAILURE_TEXT_FILES = ['crates/aura-core-base/src/route_failures.rs'];

/** Every scanned file that holds unreadable failure texts, with how many. */
export function failureTextCounts(root, folders = FAILURE_TEXT_ROOTS, extra = FAILURE_TEXT_FILES) {
  const counts = {};
  const files = folders.flatMap((folder) => rustFiles(root, folder));
  for (const file of extra) if (existsSync(join(root, file))) files.push(file);
  for (const file of files) {
    const found = unreadableFailures(readFileSync(join(root, file), 'utf8'), {
      failuresModule: file.endsWith('failures.rs'),
    });
    if (found.length) counts[file] = found.length;
  }
  return counts;
}

/** Problems against `baseline`: a file with more than it is allowed, or a count left too high. */
export function failureTextProblems(counts, baseline) {
  const problems = [];
  for (const [file, count] of Object.entries(counts)) {
    const allowed = baseline[file] ?? 0;
    if (count > allowed)
      problems.push(
        `${file}: ${count} failure texts are not \`code: 中文句\` (allowed ${allowed}); ` +
          'write the code, a colon and a Chinese sentence that says what happened and what to do',
      );
  }
  for (const [file, allowed] of Object.entries(baseline)) {
    const count = counts[file] ?? 0;
    if (count < allowed)
      problems.push(`${file}: now ${count}, lower its count in failure-text-baseline.json`);
  }
  return problems;
}
