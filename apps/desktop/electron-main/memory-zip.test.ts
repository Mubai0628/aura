// @vitest-environment node
import { expect, test } from 'vitest';
import { createHash } from 'node:crypto';
import { mkdtemp, readFile, readdir, rm, writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { tmpdir } from 'node:os';
import { execFileSync } from 'node:child_process';
import type { CoreDomainRequests } from './core-domain-requests.js';
import { buildVaultZip, writeVaultZip } from './memory-zip.js';
const files = [
  { path: 'views', directory: true, data: Buffer.alloc(0) },
  { path: '.gitignore', directory: false, data: Buffer.from('*\n') },
  {
    path: 'rules/无效.md',
    directory: false,
    data: Buffer.from('invalid frontmatter is preserved\n'),
  },
  { path: 'README.md', directory: false, data: Buffer.from('# Vault\n') },
];
const handle = {
  export_id: 'fixture',
  entries: files.length,
  total_bytes: files.reduce((n, file) => n + file.data.length, 0),
};
function source(tamper = false): Pick<CoreDomainRequests, 'memory'> {
  return {
    memory: (async (
      command: string,
      input: { index: number; offset: number; max_bytes: number },
    ) => {
      const file = files[input.index]!;
      if (command === 'memory.export.entry')
        return {
          path: file.path,
          directory: file.directory,
          length: file.data.length,
          sha256: tamper ? '0'.repeat(64) : createHash('sha256').update(file.data).digest('hex'),
        };
      const end = Math.min(file.data.length, input.offset + input.max_bytes);
      return {
        offset: input.offset,
        next_offset: end,
        eof: end === file.data.length,
        bytes: [...file.data.subarray(input.offset, end)],
      };
    }) as CoreDomainRequests['memory'],
  };
}
test('export ZIP opens independently, preserves UTF-8 and invalid files, and contains empty directories', async () => {
  const root = await mkdtemp(join(tmpdir(), 'aura-memory-zip-'));
  try {
    const destination = join(root, 'vault.zip');
    await writeVaultZip(source(), handle, destination);
    const script =
      "import zipfile,json,sys; z=zipfile.ZipFile(sys.argv[1]); assert z.testzip() is None; print(json.dumps({n:z.read(n).decode('utf-8') for n in z.namelist()},ensure_ascii=True))";
    const result = JSON.parse(
      execFileSync('python', ['-c', script, destination], { encoding: 'utf8' }),
    ) as Record<string, string>;
    expect(result['views/']).toBe('');
    expect(result['.gitignore']).toBe('*\n');
    expect(result['rules/无效.md']).toBe(files[2]!.data.toString());
    expect(await readdir(root)).toEqual(['vault.zip']);
    await writeFile(destination, 'existing archive');
    await expect(writeVaultZip(source(true), handle, destination)).rejects.toThrow('完整性');
    expect(await readFile(destination, 'utf8')).toBe('existing archive');
    expect(await readdir(root)).toEqual(['vault.zip']);
    const unsafe = {
      memory: (async () => ({
        path: '../escape',
        directory: true,
        length: 0,
        sha256: '',
      })) as CoreDomainRequests['memory'],
    };
    await expect(
      buildVaultZip(unsafe, { export_id: 'bad', entries: 1, total_bytes: 0 }),
    ).rejects.toThrow('不安全');
  } finally {
    await rm(root, { recursive: true, force: true });
  }
});
