import type { AgentMessageTextSource } from '@aura/contracts/generated';

/** The largest page the Core gives. */
const PAGE_BYTES = 65_536;

/**
 * A body read whole, page after page from its first byte. A page that does not continue the bytes already read, a failed page or
 * a body that ends short of its size fails the whole read: nothing is ever half copied.
 */
export async function readWholeText(
  sessionId: string,
  source: AgentMessageTextSource,
  totalBytes: number,
): Promise<string> {
  let text = '';
  let offset = 0;
  // Twice the pages the announced size needs, so a body that keeps growing still ends.
  for (let pages = 0; pages <= 2 * Math.ceil(totalBytes / PAGE_BYTES) + 1; pages += 1) {
    const result = await window.aura.v1.readAgentMessageText({
      session_id: sessionId,
      source,
      offset,
      limit: PAGE_BYTES,
    });
    if (!result.ok) throw new Error(result.error.message);
    const page = result.value;
    if (page.from_offset !== offset) throw new Error('the reply moved while it was read');
    text += page.text;
    if (page.next_offset === null || page.next_offset === undefined) {
      if (new TextEncoder().encode(text).length < page.total_bytes)
        throw new Error('the reply ended before its size');
      return text;
    }
    offset = page.next_offset;
  }
  throw new Error('the reply did not end');
}
