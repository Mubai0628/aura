import { expect, test } from 'vitest';
import { allowedOrigins } from './allowed-origins';
test('canonical origins preserve scheme and non-default port', () => {
  expect(
    allowedOrigins('https://EXAMPLE.com:443/\nhttp://localhost:8123\nhttps://example.com'),
  ).toEqual(['http://localhost:8123', 'https://example.com']);
});
test.each([
  'https://user:secret@example.com',
  'https://example.com/path',
  'https://example.com/?key=secret',
  'https://example.com/#part',
  'file:///secret',
])('rejects non-origin credential destination %s', (value) => {
  expect(() => allowedOrigins(value)).toThrow();
});
test('empty historical origins do not invent a permitted site', () => {
  expect(allowedOrigins('')).toEqual([]);
});
