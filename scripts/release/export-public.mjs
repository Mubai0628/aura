// Exports the public source tree of one commit and checks it before anything is published:
//
//   node scripts/release/export-public.mjs --out <new folder> [--commit HEAD] [--build]
//   node scripts/release/export-public.mjs --out <new folder> --working-tree   (a dry run)
//
// --working-tree takes the tracked and staged files as they are on disk, to check changes
// before they are committed; a published tree always comes from a commit.
//
// The tree is what public-tree.mjs selects from the commit, with public/ placed over it. Each
// check below stops the export; the findings go to <out>.report.json beside the folder:
// - credentials, by Aura's own scanner (cargo xtask scan-secrets), except the reviewed synthetic
//   test values listed in public-secrets.json;
// - local paths and personal details of the machine the tree was made on;
// - files over 8 MB;
// - comments that record stages, dates or people (comment-hygiene.mjs);
// - mentions of files and folders that stay in the development repository;
// - relative imports and Rust includes of files the tree does not hold;
// - with --build: the tree on its own, as a fresh repository, installs, compiles with every
//   feature, passes the architecture and contract checks, TypeScript, builds the named-pipe bridge,
//   passes the unit tests and leaves every committed file as it was.
import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from 'node:fs';
import { dirname, extname, join, resolve, sep } from 'node:path';
import { pathToFileURL } from 'node:url';
import { scanTree } from './comment-hygiene.mjs';
import { EXCLUDE, OVERLAY, TRANSFORMS, isPublic } from './public-tree.mjs';

const REPOSITORY = resolve(import.meta.dirname, '../..');
const LARGEST = 8 * 1024 * 1024;
const TEXT = new Set([
  '.rs',
  '.ts',
  '.tsx',
  '.mjs',
  '.cjs',
  '.js',
  '.json',
  '.toml',
  '.md',
  '.yml',
  '.yaml',
  '.css',
  '.html',
  '.ps1',
  '.psm1',
  '.txt',
  '.cs',
  '.sql',
  '.lock',
]);
// Development records. `.aura-dev` itself is a local working folder build scripts use; only its
// evidence folders are records.
const DEVELOPMENT_ONLY = [
  /\bdocs\/(v2|instruction|testing|contracts|architecture|development)\//u,
  /\bhistory\/context\b/u,
  /\.aura-dev\/(acceptance-runs|[a-z0-9-]+-run-logs)\b/u,
  /\bscripts\/s045bcdr\b/u,
];
// Files that name a development-only path on purpose: a command that refuses to run without it.
const NAMES_DEVELOPMENT_PATHS = new Set(['xtask/src/r06.rs']);

function parse(argv) {
  const options = { commit: 'HEAD', build: false, workingTree: false };
  for (let index = 0; index < argv.length; index += 1) {
    const flag = argv[index];
    if (flag === '--build' || flag === '--working-tree') {
      options[flag === '--build' ? 'build' : 'workingTree'] = true;
      continue;
    }
    const value = argv[index + 1];
    assert.ok(
      ['--out', '--commit'].includes(flag) && value,
      `unknown or incomplete option: ${flag}`,
    );
    options[flag.slice(2)] = value;
    index += 1;
  }
  assert.ok(options.out, '--out <new folder> is required');
  return options;
}

const git = (args, options = {}) =>
  execFileSync('git', args, { cwd: REPOSITORY, maxBuffer: 1024 * 1024 * 1024, ...options });

/** Tracked files of a commit: [{ mode, oid, path }]. */
function trackedFiles(commit) {
  const listing = git(['ls-tree', '-r', '-z', '--full-tree', commit]).toString('utf8');
  return listing
    .split('\0')
    .filter(Boolean)
    .map((line) => {
      const [meta, path] = line.split('\t');
      const [mode, type, oid] = meta.split(' ');
      return { mode, type, oid, path };
    });
}

/** Tracked and staged files that exist in the working tree. */
function stagedFiles() {
  return git(['ls-files', '-z', '--cached'])
    .toString('utf8')
    .split('\0')
    .filter((path) => path && existsSync(join(REPOSITORY, path)))
    .map((path) => ({ mode: '100644', type: 'blob', oid: null, path }));
}

/** Reads blobs by object id with one `git cat-file --batch`. */
function readBlobs(oids) {
  return new Promise((resolveBlobs, reject) => {
    const child = spawn('git', ['cat-file', '--batch'], { cwd: REPOSITORY });
    const blobs = new Map();
    let buffer = Buffer.alloc(0);
    let index = 0;
    child.stdout.on('data', (chunk) => {
      buffer = Buffer.concat([buffer, chunk]);
      for (;;) {
        const newline = buffer.indexOf(10);
        if (newline < 0) return;
        const header = buffer.subarray(0, newline).toString('utf8');
        const [oid, type, size] = header.split(' ');
        if (type !== 'blob') return reject(new Error(`not a blob: ${header}`));
        const length = Number(size);
        if (buffer.length < newline + 1 + length + 1) return;
        blobs.set(oid, buffer.subarray(newline + 1, newline + 1 + length));
        buffer = buffer.subarray(newline + 1 + length + 1);
        index += 1;
      }
    });
    child.on('error', reject);
    child.on('close', (code) =>
      code === 0 && index === oids.length
        ? resolveBlobs(blobs)
        : reject(new Error(`git cat-file ended with ${code} after ${index} of ${oids.length}`)),
    );
    child.stdin.end(`${oids.join('\n')}\n`);
  });
}

function walk(folder, root, out) {
  for (const name of readdirSync(folder)) {
    const path = join(folder, name);
    if (name === '.git' || name === 'node_modules' || name === 'target') continue;
    if (statSync(path).isDirectory()) walk(path, root, out);
    else
      out.push(
        path
          .slice(root.length + 1)
          .split(sep)
          .join('/'),
      );
  }
  return out;
}

const readList = (name) => JSON.parse(readFileSync(join(import.meta.dirname, name), 'utf8'));

function checkSecrets(out) {
  const result = spawnSync('cargo', ['xtask', 'scan-secrets', out], {
    cwd: REPOSITORY,
    encoding: 'utf8',
    maxBuffer: 64 * 1024 * 1024,
  });
  assert.equal(result.status, 0, `cargo xtask scan-secrets failed:\n${result.stderr}`);
  const lines = result.stdout
    .trim()
    .split('\n')
    .map((line) => JSON.parse(line));
  const summary = lines.pop();
  const reviewed = readList('public-secrets.json');
  const known = (finding) =>
    reviewed.some(
      (entry) =>
        entry.path === finding.path &&
        entry.kind === finding.kind &&
        entry.line_sha256 === finding.line_sha256,
    );
  return { summary, findings: lines.filter((finding) => !known(finding)) };
}

const escape = (text) => text.replace(/[.*+?^${}()|[\]\\]/gu, '\\$&');

/** Where this tree was made and by whom: its folder, the user's profile folder and e-mail. */
function personalRules() {
  const folder = escape(REPOSITORY).replace(/\\\\|\//gu, '[\\\\/]{1,2}');
  const rules = [new RegExp(folder, 'iu')];
  const user = process.env.USERNAME;
  if (user) rules.push(new RegExp(`[\\\\/]Users[\\\\/]{1,2}${escape(user)}\\b`, 'iu'));
  const email = spawnSync('git', ['config', 'user.email'], {
    cwd: REPOSITORY,
    encoding: 'utf8',
  }).stdout.trim();
  const local = email.split('@')[0];
  if (local && !email.endsWith('users.noreply.github.com')) {
    rules.push(new RegExp(`\\b${escape(local)}@`, 'iu'));
  }
  return rules;
}

function checkPersonal(out, files) {
  const rules = personalRules();
  const findings = [];
  for (const path of files) {
    if (!TEXT.has(extname(path)) && !path.endsWith('LICENSE')) continue;
    const lines = readFileSync(join(out, path), 'utf8').split('\n');
    lines.forEach((line, index) => {
      if (rules.some((rule) => rule.test(line))) findings.push(`${path}:${index + 1}`);
    });
  }
  return findings;
}

function checkSizes(out, files) {
  return files
    .map((path) => ({ path, bytes: statSync(join(out, path)).size }))
    .filter((file) => file.bytes > LARGEST);
}

function checkComments(out) {
  return scanTree(out);
}

const IMPORT =
  /(?:from\s+|^\s*import\s+|import\s*\(\s*|require\s*\(\s*)['"](\.{1,2}\/[^'"]+)['"]/gmu;
const INCLUDE_FILE = /include_(?:str|bytes)!\(\s*"([^"]+)"\s*\)/gu;
const SCRIPT = new Set(['.ts', '.tsx', '.mjs', '.cjs', '.js']);
// Recorded model-evaluation tasks: their graders import what the model under test writes.
const RECORDED_TASKS = /^crates\/aura-tool-eval\/fixtures\//u;
// Made by the release process, not kept in the tree: the production release root.
const MADE_AT_RELEASE = /^resources\/release-trust\/production\//u;

/** Relative imports and Rust includes that point at a file the tree does not hold. */
function checkImports(out, files) {
  const present = new Set(files);
  const exists = (path) => present.has(path) || existsSync(join(out, path));
  const findings = [];
  for (const path of files) {
    const extension = extname(path);
    if ((!SCRIPT.has(extension) && extension !== '.rs') || RECORDED_TASKS.test(path)) continue;
    const text = readFileSync(join(out, path), 'utf8');
    const folder = path.includes('/') ? path.slice(0, path.lastIndexOf('/')) : '';
    const resolve = (specifier) => {
      const parts = [];
      for (const part of `${folder}/${specifier}`.split('/')) {
        if (part === '..') parts.pop();
        else if (part && part !== '.') parts.push(part);
      }
      return parts.join('/');
    };
    if (extension === '.rs') {
      for (const [, specifier] of text.matchAll(INCLUDE_FILE)) {
        const target = resolve(specifier);
        if (!exists(target) && !MADE_AT_RELEASE.test(target)) {
          findings.push(`${path}: ${specifier}`);
        }
      }
      continue;
    }
    for (const [, written] of text.matchAll(IMPORT)) {
      // A bundler query (`?raw`) is not part of the path; packages come from the install, and a
      // template placeholder is text a generator writes, not an import of this file.
      const specifier = written.replace(/[?#].*$/u, '');
      if (specifier.includes('/node_modules/') || specifier.includes('${')) continue;
      const base = resolve(specifier);
      const stem = base.replace(/\.(js|mjs|cjs)$/u, '');
      const candidates = [
        base,
        ...['.ts', '.tsx', '.mts', '.mjs', '.js', '.cjs', '.json', '.css'].map(
          (x) => `${stem}${x}`,
        ),
        ...['/index.ts', '/index.tsx', '/index.js'].map((x) => `${base}${x}`),
      ];
      if (!candidates.some(exists)) findings.push(`${path}: ${specifier}`);
    }
  }
  return findings;
}

function checkReferences(out, files, withheld) {
  const names = [...withheld].sort((a, b) => b.length - a.length);
  const findings = [];
  for (const path of files) {
    if (!TEXT.has(extname(path)) || NAMES_DEVELOPMENT_PATHS.has(path)) continue;
    const lines = readFileSync(join(out, path), 'utf8').split('\n');
    lines.forEach((line, index) => {
      const hit =
        DEVELOPMENT_ONLY.find((rule) => rule.test(line))?.source ??
        names.find((name) => line.includes(name));
      if (hit) findings.push(`${path}:${index + 1}: ${hit}`);
    });
  }
  return findings;
}

function build(out) {
  const steps = [
    ['git', ['init', '-q']],
    ['git', ['add', '-A']],
    [
      'git',
      [
        '-c',
        'user.name=Aura',
        '-c',
        'user.email=aura@users.noreply.github.com',
        'commit',
        '-qm',
        'export',
      ],
    ],
    ['corepack', ['pnpm', 'install', '--frozen-lockfile']],
    ['cargo', ['check', '--workspace', '--all-targets', '--all-features', '--locked']],
    ['cargo', ['xtask', 'check-architecture']],
    ['cargo', ['xtask', 'contracts', '--check']],
    ['corepack', ['pnpm', 'typecheck']],
    // Some unit tests go through the named-pipe bridge, a native module built against Node's headers.
    ['corepack', ['pnpm', '--filter', '@aura/desktop', 'exec', 'node-gyp', 'install', '--ensure']],
    ['corepack', ['pnpm', '--filter', '@aura/desktop', 'build:native']],
    ['corepack', ['pnpm', 'test']],
    // Building and testing leave every committed file as it was.
    ['git', ['diff', '--exit-code']],
  ];
  const results = [];
  for (const [program, args] of steps) {
    const started = Date.now();
    // corepack is a .cmd shim on Windows, which only a command interpreter starts.
    const [file, argv] =
      process.platform === 'win32' && program === 'corepack'
        ? [process.env.ComSpec ?? 'cmd.exe', ['/d', '/c', `corepack ${args.join(' ')}`]]
        : [program, args];
    const result = spawnSync(file, argv, {
      cwd: out,
      encoding: 'utf8',
      maxBuffer: 256 * 1024 * 1024,
      env: { ...process.env, CARGO_TARGET_DIR: join(out, 'target') },
    });
    const step = `${program} ${args.join(' ')}`;
    results.push({
      step,
      status: result.status,
      seconds: Math.round((Date.now() - started) / 1000),
    });
    if (result.status !== 0) {
      writeFileSync(`${out}.build.log`, `${step}\n${result.stdout}\n${result.stderr}`);
      break;
    }
  }
  return results;
}

export async function exportPublic(options) {
  const out = resolve(options.out);
  assert.ok(!existsSync(out), `the folder already exists: ${out}`);
  const commit = git(['rev-parse', `${options.commit}^{commit}`])
    .toString('utf8')
    .trim();
  const tracked = options.workingTree ? stagedFiles() : trackedFiles(commit);
  for (const entry of tracked) {
    assert.ok(entry.type === 'blob' && entry.mode !== '120000', `not a plain file: ${entry.path}`);
  }
  const selected = new Map();
  // A file is read by its object id from a commit, or by its own path from the working tree.
  const source = (entry) => (options.workingTree ? entry.path : entry.oid);
  for (const entry of tracked) if (isPublic(entry.path)) selected.set(entry.path, source(entry));
  for (const entry of tracked) {
    if (entry.path.startsWith(OVERLAY))
      selected.set(entry.path.slice(OVERLAY.length), source(entry));
  }
  const withheld = new Set(
    tracked
      .map((entry) => entry.path)
      .filter((path) => !selected.has(path) && !path.startsWith(OVERLAY)),
  );
  const blobs = options.workingTree
    ? new Map([...selected.values()].map((path) => [path, readFileSync(join(REPOSITORY, path))]))
    : await readBlobs([...new Set(selected.values())]);
  const exported = new Set(selected.keys());
  for (const [path, oid] of selected) {
    const target = join(out, ...path.split('/'));
    mkdirSync(dirname(target), { recursive: true });
    const transform = TRANSFORMS[path];
    const bytes = blobs.get(oid);
    writeFileSync(target, transform ? transform(bytes.toString('utf8'), exported) : bytes);
  }
  const files = walk(out, out, []).sort();
  const report = {
    commit: options.workingTree ? `${commit} with working-tree changes` : commit,
    files: files.length,
    withheld: withheld.size,
    excluded_by_rule: EXCLUDE.map(([rule, why]) => ({
      rule: rule.source,
      why,
      files: [...withheld].filter((path) => rule.test(path)).length,
    })),
    secrets: checkSecrets(out),
    personal: checkPersonal(out, files),
    oversized: checkSizes(out, files),
    comments: checkComments(out),
    references: checkReferences(out, files, withheld),
    imports: checkImports(out, files),
  };
  report.passed =
    !report.secrets.findings.length &&
    !report.personal.length &&
    !report.oversized.length &&
    !report.comments.length &&
    !report.references.length &&
    !report.imports.length;
  if (options.build && report.passed) {
    report.build = build(out);
    report.passed = report.build.every((step) => step.status === 0);
  }
  writeFileSync(`${out}.report.json`, `${JSON.stringify(report, null, 2)}\n`);
  return report;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? '').href) {
  exportPublic(parse(process.argv.slice(2)))
    .then((report) => {
      console.log(
        JSON.stringify(
          {
            files: report.files,
            withheld: report.withheld,
            secrets: report.secrets.findings.length,
            personal: report.personal.length,
            oversized: report.oversized.length,
            comments: report.comments.length,
            references: report.references.length,
            imports: report.imports.length,
            build: report.build,
            passed: report.passed,
          },
          null,
          2,
        ),
      );
      process.exitCode = report.passed ? 0 : 1;
    })
    .catch((error) => {
      console.error(error.message);
      process.exit(1);
    });
}
