// @vitest-environment node
import { expect, test } from 'vitest';
import { writeProvider } from './s06-file-write-provider.mjs';
test('loopback model fixture handles auxiliary maintenance without task tools or task counters', async () => {
  const provider = await writeProvider({ path: 'unused', text: '' });
  try {
    const result = await fetch(`${provider.url}/chat/completions`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        messages: [
          {
            role: 'system',
            content: 'Return only JSON conforming to this schema (version 1): fixture',
          },
        ],
        stream: true,
      }),
    });
    const text = await result.text();
    expect(text).toContain('[DONE]');
    const data = JSON.parse(
      text
        .split('\n')
        .find((line) => line.startsWith('data: {'))!
        .slice(6),
    ) as { choices: [{ delta: { content: string } }]; usage: { total_tokens: number } };
    expect(JSON.parse(data.choices[0].delta.content)).toEqual({
      schema_version: 1,
      candidates: [],
      events: [],
    });
    expect(data.usage.total_tokens).toBe(30);
    expect(provider.requests).toEqual([]);
    expect(provider.maintenanceRequests).toHaveLength(1);
    expect(provider.errors).toEqual([]);
  } finally {
    await provider.close();
  }
});
