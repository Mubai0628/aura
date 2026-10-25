import { vi } from 'vitest';

/** Tests: a small workspace that the file routes of the desktop bridge read from. */
export const session = 'ses_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const workspace = 'wsp_01ARZ3NDEKTSV4RRFFQ69G5FAV';
export const root = 'C:\\Dev\\Demo';

const entry = (path: string, kind: 'file' | 'directory', ignored = false) => ({
  path,
  name: path.split('/').at(-1)!,
  kind,
  byte_length: kind === 'file' ? 10 : null,
  ignored,
  generated: false,
  reparse_tag: null,
});

export const directories: Record<string, ReturnType<typeof entry>[]> = {
  '.': [
    entry('README.md', 'file'),
    entry('logo.png', 'file'),
    entry('target', 'directory', true),
    entry('data.bin', 'file'),
    entry('src', 'directory'),
  ],
  src: [entry('src/main.rs', 'file')],
};

export const texts: Record<string, string> = {
  'README.md': '# 演示\n\n一个用于测试的项目。\n',
  'src/main.rs': 'fn main() {\n    println!("hello");\n}\n',
};

/** Files a glob of `**\/name` finds, by pattern. */
export const sameNames: Record<string, string[]> = {
  '**/video.html': ['baidu_video/video.html', 'old/video.html'],
};

const failure = (code: string, message = code) => ({ ok: false, error: { code, message } });

export function filesApi() {
  return {
    listWorkspaceFiles: vi.fn(async (request: { target: { path: string } }) => {
      const entries = directories[request.target.path];
      return entries
        ? { ok: true, value: { entries, catalog_revision: '1' } }
        : failure('file.not_found');
    }),
    readWorkspaceText: vi.fn(async (request: { target: { path: string } }) => {
      const text = texts[request.target.path];
      if (text === undefined)
        return failure(
          request.target.path === 'data.bin' ? 'file.encoding_ambiguous' : 'file.not_found',
        );
      const lines = text.split('\n').length;
      return {
        ok: true,
        value: {
          path: request.target.path,
          text,
          total_lines: lines,
          total_characters: text.length,
          returned_range: { range_kind: 'full' },
        },
      };
    }),
    findWorkspacePaths: vi.fn(async (request: { query: string }) => ({
      ok: true,
      value: {
        catalog_revision: '1',
        matches: request.query.includes('main')
          ? [{ path: 'src/main.rs', kind: 'file', score: 1, ignored: false, generated: false }]
          : [],
      },
    })),
    inspectWorkspaceFile: vi.fn(async (request: { target: { path: string } }) => ({
      ok: true,
      value: {
        path: request.target.path,
        byte_length: request.target.path === 'logo.png' ? 70_000 : 2048,
        media_type: request.target.path === 'logo.png' ? 'image/png' : 'application/octet-stream',
        magic_description: request.target.path === 'logo.png' ? 'PNG image' : 'ELF executable',
        is_text: false,
      },
    })),
    // Copies go to Main's clipboard.
    writeClipboard: vi.fn<(request: { text: string }) => Promise<unknown>>(async () => ({
      ok: true,
      value: { completed: true, message: '已复制' },
    })),
    // Media streams from an address Main gives; a bare name is looked for in the workspace.
    workspaceMediaUrl: vi.fn(async (request: { path: string }) => ({
      ok: true,
      url: `aura://app/workspace-media/00000000-0000-4000-8000-000000000000/${request.path.split('/').at(-1)}`,
    })),
    statWorkspaceFile: vi.fn(async (request: { target: { path: string } }) =>
      texts[request.target.path] === undefined
        ? failure('file.not_found')
        : { ok: true, value: { path: request.target.path, kind: 'file' } },
    ),
    globWorkspacePaths: vi.fn(async (request: { pattern: string }) => ({
      ok: true,
      value: { paths: sameNames[request.pattern] ?? [], catalog_revision: '1' },
    })),
    readWorkspaceBinaryRange: vi.fn(async (request: { offset: number; length: number }) => ({
      ok: true,
      value: {
        data: request.offset === 0 ? 'AAAA' : 'BBBB',
        offset: request.offset,
        returned_bytes: Math.min(request.length, 70_000 - request.offset),
        total_bytes: 70_000,
      },
    })),
  };
}

export function installFilesApi() {
  const api = filesApi();
  vi.stubGlobal('aura', { v1: api });
  return api;
}
