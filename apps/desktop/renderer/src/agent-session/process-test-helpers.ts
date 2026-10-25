import { fireEvent, waitFor } from '@testing-library/react';
import { expect } from 'vitest';

/**
 * A completed turn folds its process behind its status line. A test that
 * reads the process opens it first, as a reader would.
 */
export async function openProcesses(): Promise<void> {
  await waitFor(() => expect(document.querySelector('.agent-turn-toggle')).not.toBeNull());
  for (const toggle of document.querySelectorAll<HTMLElement>(
    '.agent-turn-toggle[aria-expanded="false"]',
  ))
    fireEvent.click(toggle);
}
