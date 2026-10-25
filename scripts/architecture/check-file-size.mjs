import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { checkFileSizes } from './file-size.mjs';

const root = resolve(import.meta.dirname, '../..');
const baseline = JSON.parse(
  readFileSync(resolve(root, 'scripts/architecture/file-size-baseline.json'), 'utf8'),
);
checkFileSizes(root, baseline);
