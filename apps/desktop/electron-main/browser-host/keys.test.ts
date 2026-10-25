// @vitest-environment node
import { describe, expect, it } from 'vitest';

import { parseKey } from './keys.js';

describe('keys the model names', () => {
  it('become the key events a page expects', () => {
    expect(parseKey('Enter')).toEqual({
      modifiers: 0,
      key: 'Enter',
      code: 'Enter',
      windowsVirtualKeyCode: 13,
      text: '\r',
    });
    expect(parseKey('ArrowLeft')).toEqual({
      modifiers: 0,
      key: 'ArrowLeft',
      code: 'ArrowLeft',
      windowsVirtualKeyCode: 37,
    });
    expect(parseKey('a')).toEqual({
      modifiers: 0,
      key: 'a',
      code: 'KeyA',
      windowsVirtualKeyCode: 65,
      text: 'a',
    });
    expect(parseKey('Shift+a')).toEqual({
      modifiers: 8,
      key: 'A',
      code: 'KeyA',
      windowsVirtualKeyCode: 65,
      text: 'A',
    });
    expect(parseKey('Space')).toEqual({
      modifiers: 0,
      key: ' ',
      code: 'Space',
      windowsVirtualKeyCode: 32,
      text: ' ',
    });
    expect(parseKey('5')).toEqual({
      modifiers: 0,
      key: '5',
      code: 'Digit5',
      windowsVirtualKeyCode: 53,
      text: '5',
    });
    expect(parseKey('F5')).toEqual({
      modifiers: 0,
      key: 'F5',
      code: 'F5',
      windowsVirtualKeyCode: 116,
    });
  });

  it('carry modifiers and type nothing under Control, Alt or Meta', () => {
    expect(parseKey('Control+a')).toEqual({
      modifiers: 2,
      key: 'a',
      code: 'KeyA',
      windowsVirtualKeyCode: 65,
    });
    expect(parseKey('Control+Shift+K')).toMatchObject({ modifiers: 10, code: 'KeyK' });
    expect(parseKey('Shift+Tab')).toEqual({
      modifiers: 8,
      key: 'Tab',
      code: 'Tab',
      windowsVirtualKeyCode: 9,
    });
    expect(parseKey('Control++')).toMatchObject({ modifiers: 2, key: '+' });
  });

  it('say what is wrong with a key they cannot press', () => {
    for (const chord of ['', 'Hyper+x', 'Foo', 'Control+'])
      expect(typeof parseKey(chord)).toBe('string');
  });
});
