/**
 * The identity line under an interruption alert.
 *
 * A stored diagnostic is one bounded line. The product sentence above it says
 * what happened; this line says which refusal, which Provider item kind and
 * which step and attempt, so a report can be matched to the journal. Only
 * whitelisted `key=value` tokens with a name-shaped value are read, so message
 * text, tool arguments or a Provider body can never reach the alert.
 */
const FIELDS = ['item', 'adapter', 'step', 'attempt'] as const;
const TOKEN = new RegExp(`(?:^|\\s)(${FIELDS.join('|')})=([A-Za-z0-9_.:@-]{1,64})`, 'gu');
const CODE = 'provider_tool_call_binding_invalid';
const MAX_LINE = 200;

export function agentDiagnosticIdentity(value: unknown): string | undefined {
  const text = value instanceof Error ? value.message : typeof value === 'string' ? value : '';
  if (!text.includes(CODE)) return undefined;
  const found = new Map<string, string>();
  for (const match of text.matchAll(TOKEN)) {
    const field = match[1];
    const token = match[2];
    if (field === undefined || token === undefined) continue;
    // The first occurrence of a field wins: a later repetition cannot rewrite it.
    if (!found.has(field)) found.set(field, `${field}=${token}`);
  }
  // A fixed order, so the line reads the same way for every refusal.
  const parts: string[] = [CODE];
  for (const field of FIELDS) {
    const part = found.get(field);
    if (part !== undefined) parts.push(part);
  }
  const line = parts.join(' · ');
  return line.length > MAX_LINE ? `${line.slice(0, MAX_LINE)}…` : line;
}
