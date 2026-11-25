import assert from 'node:assert/strict';
import { createServer } from 'node:http';
import test from 'node:test';
import { DEVELOPMENT_RENDERER_HEADER } from '../../apps/desktop/shared/development-renderer.ts';
import { rendererPortInUse, waitForDevelopmentRenderer } from './renderer.mjs';

const instance = 'a'.repeat(32);

async function listen(t, handler) {
  const server = createServer(handler);
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise((resolve) => server.close(resolve)));
  return `http://127.0.0.1:${server.address().port}`;
}

test('an occupied development port does not establish ownership of the renderer', async (t) => {
  let header;
  const origin = await listen(t, (_, res) => {
    if (header) res.setHeader(DEVELOPMENT_RENDERER_HEADER, header);
    res.end('local fixture');
  });
  assert.equal(await rendererPortInUse(origin), true);
  await assert.rejects(waitForDevelopmentRenderer(instance, { origin, timeoutMs: 100 }), /未就绪/u);
  header = 'b'.repeat(32);
  await assert.rejects(waitForDevelopmentRenderer(instance, { origin, timeoutMs: 100 }), /未就绪/u);
  header = instance;
  await waitForDevelopmentRenderer(instance, { origin, timeoutMs: 1_000 });
});
