import { describe, expect, it } from 'vitest';
import { atAllowedAt, atPath, atQuery, typedAt, withPath } from './at-token';
import { initials, matchesQuery } from './pinyin';

describe('the "@" token', () => {
  it('opens only for a typed "@" at a line start or after a space', () => {
    expect(typedAt({ inputType: 'insertText', data: '@' })).toBe(true);
    expect(typedAt({ inputType: 'insertFromPaste', data: '@' })).toBe(false);
    expect(atAllowedAt('', 0)).toBe(true);
    expect(atAllowedAt('look ', 5)).toBe(true);
    expect(atAllowedAt('one\n', 4)).toBe(true);
    expect(atAllowedAt('mail me', 7)).toBe(false);
  });

  it('follows what is typed after it until a space', () => {
    expect(atQuery('look @src/li', 5, 12)).toBe('src/li');
    expect(atQuery('look @', 5, 6)).toBe('');
    expect(atQuery('look @src next', 5, 14)).toBeUndefined();
    expect(atQuery('look src', 5, 8)).toBeUndefined();
  });

  it('writes the chosen path in its place, quoted when it has a space', () => {
    expect(atPath('src/lib.rs')).toBe('@src/lib.rs');
    expect(atPath('my docs/a b.md')).toBe('@"my docs/a b.md"');
    expect(withPath('look @sr and', 5, 8, 'src/lib.rs')).toEqual({
      value: 'look @src/lib.rs and',
      caret: 17,
    });
    expect(withPath('look @sr', 5, 8, 'my docs')).toEqual({
      value: 'look @"my docs" ',
      caret: 16,
    });
  });
});

describe('filtering', () => {
  it('finds Chinese by the first letters of its pinyin', () => {
    expect(initials('会话标题')).toBe('hhbt');
    expect(initials('Aura 设置 2')).toBe('aurasz2');
    expect(matchesQuery('会话标题', 'hhbt')).toBe(true);
    expect(matchesQuery('会话标题', 'hh')).toBe(true);
    expect(matchesQuery('会话标题', '标题')).toBe(true);
    expect(matchesQuery('会话标题', 'xyz')).toBe(false);
    expect(matchesQuery('notes.md', 'NOTE')).toBe(true);
    expect(matchesQuery('notes.md', 'nm')).toBe(false);
    expect(matchesQuery('anything', '  ')).toBe(true);
  });
});
