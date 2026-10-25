import type { AgentMessageTextSource } from '@aura/contracts/generated';
import { cleanup, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, test, vi } from 'vitest';

/**
 * The production wiring of Markdown's two modes.
 *
 * Streamdown is chosen for the shape of an answer that is still arriving, and its
 * two modes are only worth having if the caller actually selects one. `MessageText`
 * is the only production caller, so this test watches the prop it passes: a live
 * body must render in streaming mode (an unterminated construct is a body that has
 * not finished), and a sealed body must render statically (its text is final and is
 * parsed once as final text).
 *
 * The Markdown renderer is replaced here on purpose: the question is which mode the
 * caller asks for, not what Streamdown does with it, and that is already covered by
 * the Markdown tests themselves.
 */
const seen: { text: string; streaming: boolean | undefined }[] = [];

vi.mock('../markdown-body', () => ({
  MarkdownBlocks: ({ text, streaming }: { text: string; streaming?: boolean }) => {
    seen.push({ text, streaming });
    return <div data-testid="markdown">{text}</div>;
  },
}));

const { MessageText } = await import('./MessageText');

const source: AgentMessageTextSource = { event_id: 'evt_1', kind: 'user' };

/** A bridge whose one page is the whole body, so the text on screen is what was
 *  asked for and the mode decision is the only thing under test. */
function bridge(text: string) {
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: {
      v1: {
        readAgentMessageText: vi.fn(async () => ({
          ok: true,
          value: {
            next_offset: null,
            source,
            text,
            total_bytes: new TextEncoder().encode(text).length,
            body_id: null,
            from_offset: 0,
          },
        })),
      },
    },
  });
}

afterEach(() => {
  cleanup();
  seen.length = 0;
});

test('a live body is rendered in Markdown streaming mode', async () => {
  bridge('**still arriving');
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview="**still arriving"
      totalBytes={16}
      partial
      role="final"
    />,
  );
  await screen.findByTestId('markdown');
  await waitFor(() => expect(seen.length).toBeGreaterThan(0));
  expect(seen.at(-1)?.streaming).toBe(true);
});

test('a sealed body is rendered statically', async () => {
  bridge('**finished**');
  render(
    <MessageText
      sessionId="session"
      source={source}
      preview="**finished**"
      totalBytes={12}
      role="final"
    />,
  );
  await screen.findByTestId('markdown');
  await waitFor(() => expect(seen.length).toBeGreaterThan(0));
  expect(seen.at(-1)?.streaming).toBe(false);
});

test('a sealed body that is still being loaded is rendered incrementally', async () => {
  // The projection says this answer is finished, but the page carrying it never
  // arrives, so what is on screen is a prefix of a much longer document. "The model
  // has finished" and "the client has loaded all of it" are different facts, and the
  // renderer must not parse the prefix as a final document.
  Object.defineProperty(window, 'aura', {
    configurable: true,
    value: { v1: { readAgentMessageText: vi.fn(() => new Promise(() => {})) } },
  });
  render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview="**prefix"
      totalBytes={100_000}
      partial={false}
      role="final"
    />,
  );
  await waitFor(() => expect(seen.length).toBeGreaterThan(0));
  expect(seen.at(-1)?.streaming).toBe(true);
});

test('a plain-text role never reaches the Markdown renderer at all', async () => {
  bridge('reasoning text');
  render(
    <MessageText
      sessionId="session"
      source={{ kind: 'assistant', model_step_id: 'step_1' }}
      preview="reasoning text"
      totalBytes={14}
      partial
      role="reasoning"
    />,
  );
  await screen.findByText('reasoning text');
  expect(seen).toHaveLength(0);
});
