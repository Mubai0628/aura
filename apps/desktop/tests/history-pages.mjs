import { createHash } from 'node:crypto';
import { join } from 'node:path';

export const HISTORY_PAGE_CAP = 40;
export const RESULT_PAGE_CAP = 40;
export const RESULT_PAGE_LIMIT = 16_384;

/** Same namespace object the desktop release derives from the bundle namespace. */
export function namespaceObjectHash(bundleNamespace) {
  return createHash('sha256').update(String(bundleNamespace), 'utf8').digest('hex').slice(0, 32);
}

/** The v2 execution database for one startup namespace. Not a directory walk. */
export function lineageDatabasePath(localAppData, namespaceHash) {
  return join(
    localAppData,
    'Aura',
    'namespaces',
    namespaceHash,
    'data',
    'v2-execution',
    'aura-v2.db',
  );
}

function utf8Length(text) {
  return Buffer.byteLength(text, 'utf8');
}

/**
 * Reads history until the store returns a null cursor.
 * Hitting the safety cap while a next cursor is still present is incomplete.
 */
export async function loadTurns(api, page, sessionId) {
  const turns = [];
  const seen = new Set();
  const cursors = new Set();
  let before = null;
  let turnPages = 0;
  let messagePages = 0;
  let exhausted = false;
  for (let pageNo = 0; pageNo < HISTORY_PAGE_CAP; pageNo += 1) {
    if (before !== null) {
      if (cursors.has(before)) throw new Error(`history cursor repeated: ${before}`);
      cursors.add(before);
    }
    const view = await api(page, 'readAgentHistory', {
      session_id: sessionId,
      before_turn_task_id: before,
      within_turn: null,
      limit: 25,
    });
    turnPages += 1;
    for (const turn of view.turns) {
      if (seen.has(turn.turn.turn_task_id)) continue;
      seen.add(turn.turn.turn_task_id);
      turns.push(turn);
    }
    const next = view.next_before_turn_task_id ?? null;
    if (next === null) {
      exhausted = true;
      break;
    }
    if (next === before) throw new Error('history cursor did not advance');
    before = next;
  }
  if (!exhausted) throw new Error('history page walk stopped before the cursor was exhausted');
  for (const turn of turns) {
    let cursor = turn.next_messages ?? null;
    const seenCursors = new Set();
    let pagesForTurn = 0;
    while (cursor) {
      if (pagesForTurn >= HISTORY_PAGE_CAP) {
        throw new Error('message page walk stopped before the cursor was exhausted');
      }
      const key = JSON.stringify(cursor);
      if (seenCursors.has(key)) throw new Error(`message cursor repeated: ${key}`);
      seenCursors.add(key);
      const view = await api(page, 'readAgentHistory', {
        session_id: sessionId,
        before_turn_task_id: null,
        within_turn: cursor,
        limit: 25,
      });
      messagePages += 1;
      pagesForTurn += 1;
      const pageTurn = view.turns.find((item) => item.turn.turn_task_id === turn.turn.turn_task_id);
      if (!pageTurn) throw new Error('a message page did not return its turn');
      for (const message of pageTurn.user_messages) {
        if (!turn.user_messages.some((existing) => existing.event_id === message.event_id)) {
          turn.user_messages.push(message);
        }
      }
      for (const message of pageTurn.assistant_messages) {
        if (
          !turn.assistant_messages.some(
            (existing) => existing.model_step_id === message.model_step_id,
          )
        ) {
          turn.assistant_messages.push(message);
        }
      }
      const nextCursor = pageTurn.next_messages ?? null;
      if (nextCursor && JSON.stringify(nextCursor) === key) {
        throw new Error('message cursor did not advance');
      }
      cursor = nextCursor;
    }
  }
  return { turns, turnPages, messagePages, exhausted };
}

/**
 * Reads one Tool result until next_offset is null and the UTF-8 byte length
 * matches total_bytes. A safety cap, a repeated offset, or a page larger than
 * the requested limit fails before any JSON parse.
 */
export async function resultValue(api, page, sessionId, callId, limit = RESULT_PAGE_LIMIT) {
  if (!(limit >= 4 && limit <= 65_536))
    throw new Error('result page limit is outside the store range');
  let offset = 0;
  let text = '';
  const seen = new Set([0]);
  let complete = false;
  for (let pageNo = 0; pageNo < RESULT_PAGE_CAP; pageNo += 1) {
    const body = await api(page, 'readAgentResult', {
      session_id: sessionId,
      tool_call_id: callId,
      offset,
      limit,
    });
    if (typeof body.text !== 'string' || typeof body.total_bytes !== 'number') {
      throw new Error('result page text or total_bytes was not returned');
    }
    const pageBytes = utf8Length(body.text);
    if (pageBytes > limit) throw new Error('result page exceeded the requested byte limit');
    text += body.text;
    const next = body.next_offset ?? null;
    const bytes = utf8Length(text);
    if (next === null) {
      if (bytes !== body.total_bytes)
        throw new Error('result byte length did not match total_bytes');
      complete = true;
      break;
    }
    if (bytes >= body.total_bytes) {
      throw new Error('result cursor continued after the body byte length was reached');
    }
    if (next === offset || seen.has(next)) {
      throw new Error('result offset repeated before the body was complete');
    }
    if (!(next > offset)) throw new Error('result offset did not advance');
    seen.add(next);
    offset = next;
  }
  if (!complete) throw new Error('result page walk stopped before the body was exhausted');
  let parsed;
  try {
    parsed = JSON.parse(text);
  } catch {
    throw new Error('a complete result body was not valid JSON');
  }
  const outcome = parsed?.outcome;
  if (!outcome || typeof outcome !== 'object') {
    throw new Error('the Tool result has no versioned outcome envelope');
  }
  if (outcome.type !== 'succeeded') return { type: outcome.type, value: null };
  const value = outcome.payload?.value;
  if (value === undefined || value === null) {
    throw new Error('a succeeded Tool result must carry outcome.payload.value');
  }
  return { type: 'succeeded', value };
}
