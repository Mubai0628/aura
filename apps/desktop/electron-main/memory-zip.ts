//! Store-only UTF-8 ZIP using Node built-ins; no runtime or archive dependency.
import { createHash, randomBytes } from 'node:crypto';
import { open, rename, rm } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import type { MemoryExportHandle } from '@aura/contracts/generated';
import type { CoreDomainRequests } from './core-domain-requests.js';
const crcTable = Array.from({ length: 256 }, (_, value) => {
  for (let bit = 0; bit < 8; bit++) value = value & 1 ? 0xedb88320 ^ (value >>> 1) : value >>> 1;
  return value >>> 0;
});
function crc32(bytes: Buffer): number {
  let crc = 0xffffffff;
  for (const byte of bytes) crc = crcTable[(crc ^ byte) & 255]! ^ (crc >>> 8);
  return (crc ^ 0xffffffff) >>> 0;
}
function safeName(path: string, directory: boolean): Buffer {
  if (
    !path ||
    path.includes('\\') ||
    path.includes(':') ||
    path.includes('\0') ||
    path.split('/').some((p) => !p || p === '.' || p === '..')
  )
    throw new Error('导出包含不安全的相对路径');
  const name = Buffer.from(path + (directory ? '/' : ''), 'utf8');
  if (name.length > 65535) throw new Error('导出路径过长');
  return name;
}
type Source = Pick<CoreDomainRequests, 'memory'>;
/** Check the frozen source's length and hash before emitting any destination file. */
export async function buildVaultZip(source: Source, handle: MemoryExportHandle): Promise<Buffer> {
  if (handle.entries > 10000 || handle.total_bytes > 32 * 1024 * 1024)
    throw new Error('Vault 导出超过上限');
  const local: Buffer[] = [];
  const central: Buffer[] = [];
  const names = new Set<string>();
  let offset = 0;
  let total = 0;
  for (let index = 0; index < handle.entries; index++) {
    const entry = await source.memory('memory.export.entry', {
      export_id: handle.export_id,
      index,
    });
    const name = safeName(entry.path, entry.directory);
    const key = entry.path.toLocaleLowerCase('en-US');
    if (names.has(key)) throw new Error('Vault 导出路径重复');
    names.add(key);
    if (entry.length > 1048576 || !Number.isSafeInteger(entry.length) || entry.length < 0)
      throw new Error('导出文件超过 1 MiB 上限');
    if (entry.directory && entry.length !== 0) throw new Error('导出目录数据不合法');
    const parts: Buffer[] = [];
    let position = 0;
    while (position < entry.length) {
      const chunk = await source.memory('memory.export.chunk', {
        export_id: handle.export_id,
        index,
        offset: position,
        max_bytes: 65536,
      });
      if (
        chunk.offset !== position ||
        chunk.next_offset !== position + chunk.bytes.length ||
        chunk.bytes.length === 0 ||
        chunk.bytes.length > 65536 ||
        chunk.next_offset > entry.length ||
        chunk.eof !== (chunk.next_offset === entry.length)
      )
        throw new Error('Vault 导出分块不完整');
      parts.push(Buffer.from(chunk.bytes));
      position = chunk.next_offset;
    }
    const data = Buffer.concat(parts);
    total += data.length;
    if (
      total > handle.total_bytes ||
      createHash('sha256').update(data).digest('hex') !== entry.sha256
    )
      throw new Error('Vault 导出完整性校验失败');
    const crc = crc32(data);
    const header = Buffer.alloc(30);
    header.writeUInt32LE(0x04034b50);
    header.writeUInt16LE(20, 4);
    header.writeUInt16LE(0x800, 6);
    header.writeUInt16LE(33, 12);
    header.writeUInt32LE(crc, 14);
    header.writeUInt32LE(data.length, 18);
    header.writeUInt32LE(data.length, 22);
    header.writeUInt16LE(name.length, 26);
    local.push(header, name, data);
    const catalog = Buffer.alloc(46);
    catalog.writeUInt32LE(0x02014b50);
    catalog.writeUInt16LE(20, 4);
    catalog.writeUInt16LE(20, 6);
    catalog.writeUInt16LE(0x800, 8);
    catalog.writeUInt16LE(33, 14);
    catalog.writeUInt32LE(crc, 16);
    catalog.writeUInt32LE(data.length, 20);
    catalog.writeUInt32LE(data.length, 24);
    catalog.writeUInt16LE(name.length, 28);
    catalog.writeUInt32LE(entry.directory ? 0x10 : 0, 38);
    catalog.writeUInt32LE(offset, 42);
    central.push(catalog, name);
    offset += header.length + name.length + data.length;
  }
  if (total !== handle.total_bytes) throw new Error('Vault 导出大小不一致');
  const centralBytes = Buffer.concat(central);
  const end = Buffer.alloc(22);
  end.writeUInt32LE(0x06054b50);
  end.writeUInt16LE(handle.entries, 8);
  end.writeUInt16LE(handle.entries, 10);
  end.writeUInt32LE(centralBytes.length, 12);
  end.writeUInt32LE(offset, 16);
  return Buffer.concat([...local, centralBytes, end]);
}
export async function writeVaultZip(
  source: Source,
  handle: MemoryExportHandle,
  destination: string,
): Promise<void> {
  const temporary = join(
    dirname(destination),
    `.aura-memory-export-${randomBytes(16).toString('hex')}.tmp`,
  );
  let created = false;
  try {
    const bytes = await buildVaultZip(source, handle);
    const file = await open(temporary, 'wx');
    created = true;
    try {
      await file.writeFile(bytes);
      await file.sync();
    } finally {
      await file.close();
    }
    await rename(temporary, destination);
    created = false;
  } finally {
    if (created) await rm(temporary, { force: true });
  }
}
