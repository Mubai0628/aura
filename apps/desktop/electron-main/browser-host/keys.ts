/**
 * A key or chord the model names ("Enter", "ArrowLeft", "a", "Control+a", "Shift+Tab"), as
 * the DevTools key events Aura's browser sends the page. Nothing reaches the user's keyboard.
 */
export interface KeyStroke {
  /** Alt 1, Control 2, Meta 4, Shift 8 — DevTools' modifier bits. */
  modifiers: number;
  key: string;
  code: string;
  windowsVirtualKeyCode: number;
  /** What the key types, when it types anything with these modifiers. */
  text?: string;
}

const MODIFIERS: Readonly<Record<string, number>> = {
  alt: 1,
  control: 2,
  ctrl: 2,
  meta: 4,
  command: 4,
  cmd: 4,
  shift: 8,
};

const NAMED: Readonly<Record<string, [key: string, code: string, keyCode: number, text?: string]>> =
  {
    enter: ['Enter', 'Enter', 13, '\r'],
    tab: ['Tab', 'Tab', 9],
    escape: ['Escape', 'Escape', 27],
    esc: ['Escape', 'Escape', 27],
    backspace: ['Backspace', 'Backspace', 8],
    delete: ['Delete', 'Delete', 46],
    insert: ['Insert', 'Insert', 45],
    space: [' ', 'Space', 32, ' '],
    arrowleft: ['ArrowLeft', 'ArrowLeft', 37],
    arrowup: ['ArrowUp', 'ArrowUp', 38],
    arrowright: ['ArrowRight', 'ArrowRight', 39],
    arrowdown: ['ArrowDown', 'ArrowDown', 40],
    left: ['ArrowLeft', 'ArrowLeft', 37],
    up: ['ArrowUp', 'ArrowUp', 38],
    right: ['ArrowRight', 'ArrowRight', 39],
    down: ['ArrowDown', 'ArrowDown', 40],
    home: ['Home', 'Home', 36],
    end: ['End', 'End', 35],
    pageup: ['PageUp', 'PageUp', 33],
    pagedown: ['PageDown', 'PageDown', 34],
  };

const PUNCTUATION: Readonly<Record<string, [code: string, keyCode: number]>> = {
  '-': ['Minus', 189],
  '=': ['Equal', 187],
  '[': ['BracketLeft', 219],
  ']': ['BracketRight', 221],
  '\\': ['Backslash', 220],
  ';': ['Semicolon', 186],
  "'": ['Quote', 222],
  ',': ['Comma', 188],
  '.': ['Period', 190],
  '/': ['Slash', 191],
  '`': ['Backquote', 192],
};

function character(char: string): [code: string, keyCode: number] | undefined {
  if (/^[a-z]$/iu.test(char)) return [`Key${char.toUpperCase()}`, char.toUpperCase().charCodeAt(0)];
  if (/^[0-9]$/u.test(char)) return [`Digit${char}`, char.charCodeAt(0)];
  return PUNCTUATION[char] ?? (char.length === 1 && char >= ' ' ? ['', 0] : undefined);
}

/** The stroke for `chord`, or what is wrong with it. */
export function parseKey(chord: string): KeyStroke | string {
  const value = chord.trim();
  if (value === '') return '写一个按键，例如 Enter、ArrowLeft、a 或 Control+a';
  const parts =
    value === '+'
      ? ['+']
      : value.endsWith('++')
        ? [...value.slice(0, -2).split('+'), '+']
        : value.split('+');
  const name = parts.pop() ?? '';
  let modifiers = 0;
  for (const part of parts) {
    const bit = MODIFIERS[part.trim().toLowerCase()];
    if (bit === undefined) return `不认识的修饰键 ${part}；可用 Control、Shift、Alt、Meta`;
    modifiers |= bit;
  }
  const typing = (modifiers & (1 | 2 | 4)) === 0;
  const shift = (modifiers & 8) !== 0;
  const named = NAMED[name.toLowerCase()];
  if (named !== undefined) {
    const [key, code, windowsVirtualKeyCode, text] = named;
    return {
      modifiers,
      key,
      code,
      windowsVirtualKeyCode,
      ...(typing && text !== undefined ? { text } : {}),
    };
  }
  const function_ = /^f([1-9]|1[0-2])$/iu.exec(name);
  if (function_?.[1] !== undefined) {
    const n = Number(function_[1]);
    return { modifiers, key: `F${n}`, code: `F${n}`, windowsVirtualKeyCode: 111 + n };
  }
  if ([...name].length !== 1)
    return `不认识的按键 ${name}；可用 Enter、Tab、Escape、方向键、Space、F1–F12 或单个字符`;
  const known = character(name);
  if (known === undefined) return `不认识的按键 ${name}`;
  const [code, windowsVirtualKeyCode] = known;
  const key = shift && /^[a-z]$/u.test(name) ? name.toUpperCase() : name;
  return { modifiers, key, code, windowsVirtualKeyCode, ...(typing ? { text: key } : {}) };
}
