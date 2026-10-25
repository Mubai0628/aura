import { spawnSync } from 'node:child_process';
import {
  closeSync,
  mkdirSync,
  mkdtempSync,
  openSync,
  readFileSync,
  rmSync,
  rmdirSync,
} from 'node:fs';
import { resolve } from 'node:path';

// File-backed output also works on Windows hosts that cannot create anonymous child pipes.
export function capture(root, program, args) {
  const cache = resolve(root, 'target/architecture');
  mkdirSync(cache, { recursive: true });
  const directory = mkdtempSync(resolve(cache, 'capture-'));
  const path = resolve(directory, 'stdout');
  const output = openSync(path, 'wx');
  try {
    const result = spawnSync(program, args, { cwd: root, stdio: ['inherit', output, 'inherit'] });
    if (result.error) throw result.error;
    if (result.status !== 0)
      throw new Error(`${program} ${args.join(' ')} failed (${result.status})`);
    return readFileSync(path);
  } finally {
    closeSync(output);
    rmSync(path);
    rmdirSync(directory);
  }
}
