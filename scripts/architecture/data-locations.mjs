// Where Aura's data, logs, global memory and window profile are is worked out in one place, the
// `aura-data-location` crate; the desktop takes them from the launcher or from Core. Product code
// elsewhere must not read the profile folders from the environment or put these paths together
// itself, or a moved installation would find half of its data in the old place.
import { readFileSync } from 'node:fs';
import { productionSource, sourceFiles } from './file-size.mjs';

const QUOTE = '["\'`]';

const RUST_PATTERNS = [
  ['environment', /\bvar(?:_os)?\(\s*"(?:LOCAL)?APPDATA"\s*\)/u],
  ['data folder', /"Aura[\\/]+(?:namespaces|logs)\b/u],
  ['data folder', /\.join\(\s*"Aura"\s*\)\s*\.join\(\s*"(?:namespaces|logs)"\s*\)/u],
  ['global memory', /"\.aura[\\/]+memories\b/u],
  ['global memory', /\.join\(\s*"\.aura"\s*\)\s*\.join\(\s*"memories"\s*\)/u],
  ['window profile', /"@aura[\\/]+desktop\b/u],
  ['window profile', /\.join\(\s*"@aura"\s*\)\s*\.join\(\s*"desktop"\s*\)/u],
];

const SCRIPT_PATTERNS = [
  [
    'environment',
    new RegExp(
      `process\\.env(?:\\.(?:LOCAL)?APPDATA\\b|\\[\\s*${QUOTE}(?:LOCAL)?APPDATA${QUOTE}\\s*\\])`,
      'u',
    ),
  ],
  ['environment', /getPath\(\s*['"`]appData['"`]\s*\)/u],
  ['data folder', new RegExp(`${QUOTE}Aura[\\\\/]+(?:namespaces|logs)\\b`, 'u')],
  [
    'data folder',
    new RegExp(`${QUOTE}Aura${QUOTE}\\s*,\\s*${QUOTE}(?:namespaces|logs)${QUOTE}`, 'u'),
  ],
  ['global memory', new RegExp(`${QUOTE}\\.aura[\\\\/]+memories\\b`, 'u')],
  ['global memory', new RegExp(`${QUOTE}\\.aura${QUOTE}\\s*,\\s*${QUOTE}memories${QUOTE}`, 'u')],
  ['window profile', new RegExp(`${QUOTE}@aura[\\\\/]+desktop\\b`, 'u')],
  ['window profile', new RegExp(`${QUOTE}@aura${QUOTE}\\s*,\\s*${QUOTE}desktop${QUOTE}`, 'u')],
];

/**
 * The source with its comments blanked out (line breaks kept, so line numbers still match).
 * Strings stay: Rust's ordinary and raw strings, and in scripts single, double and template
 * literals, so a `//` inside one is not taken for a comment.
 */
export function stripComments(source, language) {
  let out = '';
  let index = 0;
  const blank = (text) => text.replace(/[^\n]/gu, ' ');
  while (index < source.length) {
    const rest = source.slice(index);
    if (rest.startsWith('//')) {
      const end = rest.indexOf('\n');
      const comment = end === -1 ? rest : rest.slice(0, end);
      out += blank(comment);
      index += comment.length;
      continue;
    }
    if (rest.startsWith('/*')) {
      const end = rest.indexOf('*/', 2);
      const comment = end === -1 ? rest : rest.slice(0, end + 2);
      out += blank(comment);
      index += comment.length;
      continue;
    }
    const raw = language === 'rust' ? /^r(#*)"/u.exec(rest) : null;
    if (raw !== null) {
      const close = `"${raw[1]}`;
      const end = rest.indexOf(close, raw[0].length);
      const literal = end === -1 ? rest : rest.slice(0, end + close.length);
      out += literal;
      index += literal.length;
      continue;
    }
    const quote = rest[0];
    const quotes = language === 'rust' ? ['"'] : ['"', "'", '`'];
    if (quotes.includes(quote) && !(language === 'rust' && /^b?'/u.test(rest))) {
      let end = 1;
      while (end < rest.length && rest[end] !== quote) end += rest[end] === '\\' ? 2 : 1;
      const literal = rest.slice(0, end + 1);
      out += literal;
      index += literal.length;
      continue;
    }
    out += quote;
    index += 1;
  }
  return out;
}

/**
 * Rust code (comments already blanked) with each inline `#[cfg(test)] mod … { … }` blanked too:
 * tests may build any path they like. Strings and character literals are skipped while the
 * braces are counted.
 */
export function stripTestModules(code) {
  const opening = /#\[cfg\(test\)\]\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+\w+\s*\{/gu;
  let out = '';
  let from = 0;
  for (let match = opening.exec(code); match !== null; match = opening.exec(code)) {
    if (match.index < from) continue;
    let index = match.index + match[0].length;
    let depth = 1;
    while (index < code.length && depth > 0) {
      const rest = code.slice(index);
      const raw = /^r(#*)"/u.exec(rest);
      const literal = raw
        ? rest.slice(0, rest.indexOf(`"${raw[1]}`, raw[0].length) + raw[1].length + 1)
        : (/^"(?:\\.|[^\\"])*"/su.exec(rest)?.[0] ?? /^'(?:\\.|[^\\'])'/u.exec(rest)?.[0]);
      if (literal !== undefined && literal.length > 0) {
        index += literal.length;
        continue;
      }
      if (rest[0] === '{') depth += 1;
      if (rest[0] === '}') depth -= 1;
      index += 1;
    }
    out += code.slice(from, match.index) + code.slice(match.index, index).replace(/[^\n]/gu, ' ');
    from = index;
    opening.lastIndex = index;
  }
  return out + code.slice(from);
}

/** Each place in `source` that works a location out itself, with its line number. */
export function locationViolations(path, source) {
  const language = path.endsWith('.rs') ? 'rust' : 'script';
  const stripped = stripComments(source, language);
  const code = language === 'rust' ? stripTestModules(stripped) : stripped;
  const patterns = language === 'rust' ? RUST_PATTERNS : SCRIPT_PATTERNS;
  return patterns.flatMap(([kind, pattern]) => {
    const global = new RegExp(pattern.source, `${pattern.flags}g`);
    return [...code.matchAll(global)].map((match) => ({
      path,
      line: code.slice(0, match.index).split('\n').length,
      kind,
      text: match[0],
    }));
  });
}

/** The product sources the rule covers. */
export function coveredSource(path) {
  if (!productionSource(path)) return false;
  const rust = /^crates\/([^/]+)\/src\/.+\.rs$/u.exec(path);
  if (rust !== null) return !['aura-data-location', 'aura-tool-eval'].includes(rust[1]);
  return (
    /^apps\/desktop\/electron-main\/.+\.(?:ts|mts|mjs)$/u.test(path) &&
    !/\.test\.[cm]?[jt]s$/u.test(path) &&
    !path.split('/').some((part) => part.endsWith('-tests'))
  );
}

export function checkDataLocations(root) {
  const found = sourceFiles(root)
    .filter(coveredSource)
    .flatMap((path) => locationViolations(path, readFileSync(`${root}/${path}`, 'utf8')));
  if (found.length) {
    throw new Error(
      `data locations are worked out only by aura-data-location:\n${found
        .map(({ path, line, kind, text }) => `${path}:${line}: ${kind}: ${text}`)
        .join('\n')}`,
    );
  }
}
