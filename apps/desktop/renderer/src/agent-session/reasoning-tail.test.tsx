import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';
import { ReasoningTrace } from './reasoning-trace';

// "思考的收缩状态小的那一行预览……改为持续加载模型最新生产的内容，而不是一直不变的
// 最开始生成的部分" — closed, a thought shows its newest line: while it is written, as it grows; once
// written, its last.

afterEach(cleanup);

const bytes = (text: string) => new TextEncoder().encode(text).length;

/** The thought's whole body as the Core would page it from any byte. */
function thoughtBody(body: { text: string }) {
  const readAgentMessageText = vi.fn(async (request: { offset: number; limit: number }) => {
    const encoded = new TextEncoder().encode(body.text);
    const slice = encoded.slice(request.offset, request.offset + request.limit);
    return {
      ok: true,
      value: {
        source: { kind: 'reasoning', model_step_id: 'step' },
        text: new TextDecoder().decode(slice),
        total_bytes: encoded.length,
        from_offset: request.offset,
        next_offset: null,
        body_id: 'live',
      },
    };
  });
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText } },
  });
  return readAgentMessageText;
}

const line = () => document.querySelector('.agent-reasoning-preview')?.textContent;

test('closed, a whole short thought shows its last line, read from what is already here', () => {
  const read = thoughtBody({ text: '' });
  const preview = '先看结构。\n再写页面。';
  render(
    <ReasoningTrace
      sessionId="session"
      modelStepId="step"
      preview={preview}
      totalBytes={bytes(preview)}
      partial={false}
      bodyId={null}
    />,
  );
  expect(line()).toBe('再写页面。');
  expect(read).not.toHaveBeenCalled();
});

test('a thought longer than its preview shows the newest line, and follows it as it grows', async () => {
  const body = { text: `${'起'.repeat(2000)}\n先读文件。\n然后改写函数。` };
  const read = thoughtBody(body);
  const preview = '起'.repeat(100);
  const view = render(
    <ReasoningTrace
      sessionId="session"
      modelStepId="step"
      preview={preview}
      totalBytes={bytes(body.text)}
      partial
      generating
      bodyId="live"
    />,
  );
  await waitFor(() => expect(line()).toBe('然后改写函数。'));
  expect(read.mock.calls[0]![0]).toMatchObject({
    source: { kind: 'reasoning', model_step_id: 'step' },
  });
  expect(read.mock.calls[0]![0].offset).toBeGreaterThan(0);
  body.text += '\n最后跑测试确认。';
  view.rerender(
    <ReasoningTrace
      sessionId="session"
      modelStepId="step"
      preview={preview}
      totalBytes={bytes(body.text)}
      partial
      generating
      bodyId="live"
    />,
  );
  await waitFor(() => expect(line()).toBe('最后跑测试确认。'));
  expect(screen.getByText('思考过程')).toBeTruthy();
});
