import assert from 'node:assert/strict';
import test from 'node:test';
import { ESLint } from 'eslint';

const eslint = new ESLint();
const filePath = 'apps/desktop/renderer/src/architecture-boundary.ts';

test('Renderer rejects Node builtins and Electron through static and dynamic imports', async () => {
  for (const name of ['electron', 'http', 'net', 'os', 'node:fs', 'fs/promises']) {
    for (const text of [`import '${name}';`, `void import('${name}');`]) {
      const [result] = await eslint.lintText(text, { filePath });
      assert.ok(
        result.messages.some((message) =>
          ['no-restricted-imports', 'no-restricted-syntax'].includes(message.ruleId),
        ),
        text,
      );
    }
  }
});

test('Renderer can import its projection store', async () => {
  const [result] = await eslint.lintText("import './core-health-store';", { filePath });
  assert.equal(result.errorCount, 0);
});
