import { appendFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

/** Appends one event to the namespace's `logs/main.jsonl`, Main's own diagnostic log. */
export async function appendMainLog(
  localAppData: string,
  namespaceObjectHash: string,
  event: string,
  fields: Record<string, unknown>,
): Promise<void> {
  const path = resolve(localAppData, 'Aura/namespaces', namespaceObjectHash, 'logs/main.jsonl');
  await mkdir(dirname(path), { recursive: true });
  await appendFile(
    path,
    `${JSON.stringify({ component: 'electron_main', event, fields, timestamp: new Date().toISOString() })}\n`,
    'utf8',
  );
}
