import { appendFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

/** Appends one event to `logs/main.jsonl` in the installation's data folder, Main's own log. */
export async function appendMainLog(
  dataRoot: string,
  event: string,
  fields: Record<string, unknown>,
): Promise<void> {
  const path = resolve(dataRoot, 'logs/main.jsonl');
  await mkdir(dirname(path), { recursive: true });
  await appendFile(
    path,
    `${JSON.stringify({ component: 'electron_main', event, fields, timestamp: new Date().toISOString() })}\n`,
    'utf8',
  );
}
