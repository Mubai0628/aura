import { appendFileSync, mkdirSync } from 'node:fs';
import { appendFile, mkdir } from 'node:fs/promises';
import { dirname, resolve } from 'node:path';

/**
 * Main's log, `logs/main.jsonl` in the installation's data folder, known before the first line.
 * Sync logging is reserved for uncaught errors, including the last turn before exit.
 */
export function createMainLogger(dataRoot: string) {
  const path = () => resolve(dataRoot, 'logs/main.jsonl');
  const record = (event: string, fields: Record<string, unknown>) =>
    `${JSON.stringify({ component: 'electron_main', event, fields, timestamp: new Date().toISOString() })}\n`;
  return {
    path,
    async append(event: string, fields: Record<string, unknown>) {
      const file = path();
      await mkdir(dirname(file), { recursive: true });
      await appendFile(file, record(event, fields), 'utf8');
    },
    appendSync(event: string, fields: Record<string, unknown>) {
      const file = path();
      mkdirSync(dirname(file), { recursive: true });
      appendFileSync(file, record(event, fields), 'utf8');
    },
  };
}
