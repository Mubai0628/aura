import { expect, test } from 'vitest';
import { MAX_QUOTE_CHARS, quoteFromSelection, quotedMessage } from './quote-text';

// A passage replied to with 回复 goes before the typed words as a Markdown quote, with its reply
// under it.

const block = (quote: string, reply = '') => ({ id: quote, quote, reply });

test('the quotes go before the typed words, each a Markdown quote with its reply under it', () => {
  expect(
    quotedMessage([block('第一行\n第二行', '这里要改'), block('另一段')], '按这两处修改'),
  ).toBe('> 第一行\n> 第二行\n\n这里要改\n\n> 另一段\n\n按这两处修改');
});

test('an empty reply leaves the quote alone, and with nothing typed the quotes are the message', () => {
  expect(quotedMessage([block('只引用这一句')], '')).toBe('> 只引用这一句');
  expect(quotedMessage([block('甲', '  '), block('乙', '\n回复乙\n')], '  \n')).toBe(
    '> 甲\n\n> 乙\n\n回复乙',
  );
});

test('without quotes the typed words go exactly as typed', () => {
  expect(quotedMessage([], '  原样\n发送 ')).toBe('  原样\n发送 ');
});

test('a blank line inside a quote stays inside the quote', () => {
  expect(quotedMessage([block('第一段\n\n第二段')], '')).toBe('> 第一段\n>\n> 第二段');
});

test('a selection is quoted as read, with line ends, trailing spaces and blank runs tidied', () => {
  expect(quoteFromSelection('\r\n  \r\n第一段  \r\n\r\n\r\n\r\n  第二段\t\r\n\r\n')).toBe(
    '第一段\n\n  第二段',
  );
  expect(quoteFromSelection(' \n\t\n')).toBeUndefined();
});

test('a long selection is quoted up to 2000 characters and ends with an ellipsis', () => {
  expect(MAX_QUOTE_CHARS).toBe(2000);
  const quoted = quoteFromSelection('长'.repeat(2500))!;
  expect(Array.from(quoted)).toHaveLength(2001);
  expect(quoted).toBe(`${'长'.repeat(2000)}…`);
  // Characters, not UTF-16 units: an emoji is never cut in half.
  expect(quoteFromSelection('😀'.repeat(2001))).toBe(`${'😀'.repeat(2000)}…`);
  // At the bound nothing is cut.
  expect(quoteFromSelection('字'.repeat(2000))).toBe('字'.repeat(2000));
});
