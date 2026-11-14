/**
 * Main's own sentence from an error thrown in a Main handler. Electron wraps it on the way back to
 * the window ("Error invoking remote method '<channel>': Error: <sentence>"); the wrapping is not
 * for the user.
 */
export function mainSentence(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/^Error invoking remote method '[^']*': (?:Error: )?/u, '');
}
