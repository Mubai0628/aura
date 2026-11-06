/**
 * Filtering by what was typed, Chinese by the first letters of its pinyin too: “会话标题” is
 * found by “hhbt”. The letters come from the browser's own Chinese collation, which sorts by
 * pinyin; each letter starts at the first character that sorts there.
 */
const LETTERS = 'abcdefghjklmnopqrstwxyz';
const STARTS = '阿八嚓哒妸发旮哈讥咔垃痳拏噢妑七呥扨它穵夕丫帀';
const HAN = /\p{Script=Han}/u;

let collator: Intl.Collator | undefined;
const pinyinOrder = () => (collator ??= new Intl.Collator('zh-Hans-CN-u-co-pinyin'));

/** The first letter of one Chinese character's pinyin; undefined for anything else. */
function initial(character: string): string | undefined {
  if (!HAN.test(character)) return undefined;
  const order = pinyinOrder();
  let found: string | undefined;
  for (let index = 0; index < STARTS.length; index += 1) {
    if (order.compare(STARTS[index]!, character) <= 0) found = LETTERS[index];
    else break;
  }
  return found;
}

/** `text` as its letters and digits, each Chinese character as the first letter of its pinyin. */
export function initials(text: string): string {
  let out = '';
  for (const character of text) {
    const letter = initial(character);
    if (letter) out += letter;
    else if (/[\p{L}\p{N}]/u.test(character)) out += character.toLowerCase();
  }
  return out;
}

/** Whether `label` contains what was typed, or its pinyin initials do. */
export function matchesQuery(label: string, query: string): boolean {
  const typed = query.trim().toLowerCase();
  if (!typed) return true;
  if (label.toLowerCase().includes(typed)) return true;
  return HAN.test(label) && initials(label).includes(typed);
}
